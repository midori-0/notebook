//! `rmid`: removing identifiers, from chosen notes or from the notebook altogether.

use crate::Res;
use crate::env::Env;
use crate::filter::{Filter, Numbers};
use crate::format;
use crate::link;
use crate::paths::{canon, folder};
use crate::store::{self, Lock, Rewrite};
use std::fs;
use std::io::Write;
use std::path::Path;

/// One identifier `rmid` took off.
#[derive(Debug)]
pub struct Removed {
    pub id: String,
    /// Its meaning, when it was deleted from the dictionary too.
    pub deleted: Option<String>,
    /// How many notes carried it.
    pub notes: usize,
}

/// Removes identifiers from notes `ns`, or from every note and the dictionary when `ns` is empty.
/// Timestamps stay as they are: this changes the vocabulary, not what the notes say. A registered
/// link goes if it filters on a deleted identifier, or if it would select nothing afterwards.
/// Returns what was removed and the deleted links' paths.
pub fn rmid(env: &Env, path: &Path, ids: &[String], ns: &[u32]) -> Res<(Vec<Removed>, Vec<String>)> {
    let mut wanted: Vec<&str> = vec![];
    for id in ids {
        if !wanted.contains(&id.as_str()) {
            wanted.push(id);
        }
    }
    let _lock = Lock::wait(&env.cache, path)?;
    let (head, mut notes) = store::open(path)?;
    if let Some(id) = wanted.iter().find(|id| !head.ids().any(|(k, _)| k == **id)) {
        return Err(format!("no id '{id}'").into());
    }
    let everywhere = ns.is_empty();
    let chosen = Numbers::new(ns);
    let gone = |id: &str| wanted.contains(&id);
    let affects = |n: u32| everywhere || chosen.has(n);
    let abs = canon(path)?;
    let dir = folder(&abs)?;

    // Pass 1: count, check the notes exist, and see what each link selects before and after.
    let mut watches: Vec<Watch> = head
        .links
        .iter()
        .filter_map(|rel| Some(Watch { rel, filter: link::resolve(&dir.join(rel)).ok()?.1, before: false, after: false }))
        .collect();
    let mut counts = vec![0; wanted.len()];
    let mut seen = vec![];
    while let Some(note) = notes.next_note()? {
        let hit = affects(note.n);
        if hit {
            seen.push(note.n);
            for (count, id) in counts.iter_mut().zip(&wanted) {
                *count += note.has_id(id) as usize;
            }
        }
        for w in &mut watches {
            let before = w.filter.matches(&note);
            w.before |= before;
            w.after |= if hit { w.filter.matches_with(&note, |i| note.has_id(i) && !gone(i)) } else { before };
        }
    }
    if let Some(n) = ns.iter().find(|n| !seen.contains(n)) {
        return Err(format!("no note {n}").into());
    }
    let removed = wanted.iter().zip(&counts).map(|(id, &notes)| Removed {
        id: id.to_string(),
        deleted: everywhere.then(|| head.ids().find(|(k, _)| k == id).map_or("", |(_, v)| v).to_string()),
        notes,
    });
    let removed: Vec<Removed> = removed.collect();

    let (mut kept, mut deleted) = (vec![], vec![]);
    for w in &watches {
        let names_deleted = everywhere && w.filter.ids.iter().any(|i| gone(i));
        if names_deleted || (w.before && !w.after) {
            deleted.push(w.rel.clone());
        } else {
            kept.push(w.rel.as_str());
        }
    }
    if !everywhere && counts.iter().all(|&c| c == 0) {
        return Ok((removed, deleted)); // nothing to change
    }

    // Pass 2: rewrite, retagging only the notes that carry a removed identifier.
    notes.rewind()?;
    let mut out = Rewrite::create(path)?;
    let dict = head.ids().filter(|(k, _)| !(everywhere && gone(k)));
    format::write_header(&mut out, &store::uid(&head, &abs)?, dict, head.last, kept)?;
    while let Some(note) = notes.next_note()? {
        if affects(note.n) && note.ids().any(gone) {
            note.retagged(&mut out, |i| !gone(i))?;
        } else {
            out.write_all(note.raw.as_bytes())?;
        }
    }
    drop(notes);
    out.commit()?;
    for rel in &deleted {
        fs::remove_file(dir.join(rel))?;
    }
    Ok((removed, deleted))
}

/// A registered link, and whether it selects any note before and after the change.
struct Watch<'a> {
    rel: &'a String,
    filter: Filter,
    before: bool,
    after: bool,
}
