//! Removing notes and getting them back: `rm`, `restore` and `purge`.

use crate::Res;
use crate::env::Env;
use crate::filter::Filter;
use crate::format;
use crate::link;
use crate::paths::{canon, folder};
use crate::store::{self, Lock, Rewrite};
use crate::trash::{Incoming, Trash};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Removes notes. A registered link goes too if it names a removed note, or if everything it
/// selects is being removed. Notes and links reach the trash before the notebook changes, so a
/// crash can leave a duplicate but never lose anything. Returns the deleted links' paths.
pub fn rm(env: &Env, path: &Path, ns: &[u32]) -> Res<Vec<String>> {
    let _lock = Lock::wait(&env.cache, path)?;
    let (head, mut notes) = store::open(path)?;
    let abs = canon(path)?;
    let dir = folder(&abs)?;
    let removing = Numbers::new(ns);

    // Pass 1: copy out the notes being removed, and watch what each link selects. A link that
    // no longer reads (deleted by hand) just gets unregistered.
    let mut watches: Vec<Watch> = head
        .links
        .iter()
        .filter_map(|rel| Some(Watch { rel, filter: link::resolve(&dir.join(rel)).ok()?.1, first: None, only_removed: true }))
        .collect();
    let mut incoming = Incoming::default();
    let mut last = head.last;
    while let Some(note) = notes.next_note()? {
        last = last.max(note.n);
        let goes = removing.has(note.n);
        if goes {
            incoming.notes.push_str(note.raw);
            incoming.numbers.push(note.n);
        }
        for w in watches.iter_mut().filter(|w| w.filter.matches(&note)) {
            w.first.get_or_insert(note.n);
            w.only_removed &= goes;
        }
    }
    if let Some(n) = ns.iter().find(|n| !incoming.numbers.contains(n)) {
        return Err(format!("no note {n}").into());
    }
    let (mut kept, mut gone) = (vec![], vec![]);
    for w in &watches {
        let named = w.filter.notes.iter().copied().find(|&n| removing.has(n));
        match named.or(w.first.filter(|_| w.only_removed)) {
            Some(n) => gone.push((n, w.rel)),
            None => kept.push(w.rel.as_str()),
        }
    }

    let uid = store::uid(&head, &abs)?;
    let trash = Trash::of(&env.cache, &uid);
    fs::create_dir_all(trash.dir())?;
    let now = env.stamp();
    let compact = now.replace(':', "");
    for (i, (n, rel)) in gone.iter().enumerate() {
        let saved = format!("{n}-{i}-{compact}.nt");
        fs::copy(dir.join(rel), trash.dir().join(&saved))?;
        incoming.links.push(format!("{n}\t{rel}\t{saved}"));
    }
    trash.update(trash.read(&[])?, Some(&abs), &now, &env.cutoff(), |_| true, incoming)?;

    // Pass 2: rewrite the notebook without them. `last` keeps their numbers from coming back.
    notes.rewind()?;
    let mut out = Rewrite::create(path)?;
    format::write_header(&mut out, &uid, head.ids(), last, kept)?;
    while let Some(note) = notes.next_note()? {
        if !removing.has(note.n) {
            out.write_all(note.raw.as_bytes())?;
        }
    }
    drop(notes);
    out.commit()?;
    for (_, rel) in &gone {
        fs::remove_file(dir.join(rel))?;
    }
    Ok(gone.into_iter().map(|(_, rel)| rel.clone()).collect())
}

/// A registered link, and what pass 1 of `rm` learns about its selection.
struct Watch<'a> {
    rel: &'a String,
    filter: Filter,
    /// The first note it selects.
    first: Option<u32>,
    /// Whether every note it selects is being removed.
    only_removed: bool,
}

/// Puts notes back with their original numbers, and recreates the links deleted with them.
/// Returns each link's path, and whether a file already stood there (it's kept, not replaced).
pub fn restore(env: &Env, path: &Path, ns: &[u32]) -> Res<Vec<(String, bool)>> {
    let _lock = Lock::wait(&env.cache, path)?;
    let (head, mut notes) = store::open(path)?;
    let abs = canon(path)?;
    let uid = store::uid(&head, &abs)?;
    let trash = Trash::of(&env.cache, &uid);
    let snap = trash.read(ns)?;
    let back = snap.links_of(ns);
    let restoring = Numbers::new(ns);

    // Notebooks stay in number order, so one merging pass puts each note back in its place.
    let mut out = Rewrite::create(path)?;
    let new_links = back.iter().map(|(rel, _)| rel.as_str()).filter(|r| !head.links.iter().any(|l| l == r));
    format::write_header(&mut out, &uid, head.ids(), head.last, head.links().chain(new_links))?;
    let mut pending = snap.wanted.iter().peekable();
    let mut clashes = vec![];
    while let Some(note) = notes.next_note()? {
        while let Some((_, raw)) = pending.next_if(|(n, _)| *n < note.n) {
            out.write_all(raw.as_bytes())?;
        }
        if restoring.has(note.n) {
            clashes.push(note.n);
        }
        out.write_all(note.raw.as_bytes())?;
    }
    for (_, raw) in pending {
        out.write_all(raw.as_bytes())?;
    }
    drop(notes);
    for &n in ns {
        if !snap.has(n) {
            return Err(format!("note {n} is not in the trash").into());
        }
        if clashes.contains(&n) {
            return Err(format!("note {n} already exists in the notebook").into());
        }
    }
    out.commit()?;

    let dir = folder(&abs)?;
    let mut restored = Vec::with_capacity(back.len());
    for (rel, saved) in back {
        let dest = dir.join(&rel);
        let existed = dest.exists();
        if !existed {
            fs::create_dir_all(dest.parent().unwrap_or(dir))?;
            fs::copy(trash.dir().join(&saved), &dest)?;
        }
        restored.push((rel, existed));
    }
    // Drops the restored entries and their saved links, plus anything expired.
    trash.update(snap, Some(&abs), &env.stamp(), &env.cutoff(), |n| !restoring.has(n), Incoming::default())?;
    Ok(restored)
}

/// Deletes notes from the trash for good; no numbers means all of them. Returns how many.
pub fn purge(env: &Env, path: &Path, ns: &[u32]) -> Res<usize> {
    let _lock = Lock::wait(&env.cache, path)?;
    let trash = Trash::of(&env.cache, &store::uid(&store::header(path)?, path)?);
    let snap = trash.read(&[])?;
    if let Some(n) = ns.iter().find(|&&n| !snap.has(n)) {
        return Err(format!("note {n} is not in the trash").into());
    }
    let purging = Numbers::new(ns);
    trash.update(snap, Some(path), &env.stamp(), "", |n| !ns.is_empty() && !purging.has(n), Incoming::default())
}

/// Note numbers, sorted so a lookup stays cheap however many are given.
struct Numbers(Vec<u32>);

impl Numbers {
    fn new(ns: &[u32]) -> Self {
        let mut v = ns.to_vec();
        v.sort_unstable();
        v.dedup();
        Numbers(v)
    }

    fn has(&self, n: u32) -> bool {
        self.0.binary_search(&n).is_ok()
    }
}
