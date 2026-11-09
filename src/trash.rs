//! The trash: removed notes, and the links deleted with them, kept until they expire.
//!
//! Each notebook's trash is a folder in the cache named after its uid, so it follows the notebook
//! when it moves. The folder holds `trash.nb`, in notebook format, and saved copies of deleted
//! `.nt` links. `trash.nb` has its own header lines:
//!
//! - `notebook <path>`: where the owner was last seen, so trash of deleted notebooks can be found;
//! - `removed <n> <timestamp>`: when each note was removed;
//! - `link <n>\t<path relative to the notebook>\t<saved copy>`: a deleted link and its note.

use crate::Res;
use crate::env::Env;
use crate::format::{Header, MAGIC};
use crate::paths::canon;
use crate::store::{self, Lock, Notes, Rewrite};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Trash {
    dir: PathBuf,
}

/// Notes and links on their way into the trash.
#[derive(Default)]
pub struct Incoming {
    /// The notes as stored, back to back.
    pub notes: String,
    pub numbers: Vec<u32>,
    /// Link entries, `<n>\t<rel>\t<saved>`.
    pub links: Vec<String>,
}

impl Incoming {
    fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.numbers.is_empty() && self.links.is_empty()
    }
}

/// The trash as read at one moment.
pub struct Snapshot {
    head: Header,
    notes: Option<Notes>,
    /// Every note number, in file order.
    pub numbers: Vec<u32>,
    /// Copies of the notes asked for, sorted by number.
    pub wanted: Vec<(u32, String)>,
}

impl Snapshot {
    pub fn has(&self, n: u32) -> bool {
        self.numbers.contains(&n)
    }

    /// The saved links that went with notes `ns`, as (path relative to the notebook, saved copy).
    pub fn links_of(&self, ns: &[u32]) -> Vec<(String, String)> {
        let of_ns = self.head.links.iter().filter_map(|l| entry(l)).filter(|(n, ..)| ns.contains(n));
        of_ns.map(|(_, rel, saved)| (rel.to_owned(), saved.to_owned())).collect()
    }
}

impl Trash {
    pub fn of(cache: &Path, uid: &str) -> Trash {
        Trash { dir: cache.join(uid) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The trash file's header and notes; `None` when there's no trash.
    pub fn open(&self) -> Res<Option<(Header, Notes)>> {
        match File::open(self.dir.join("trash.nb")) {
            Ok(f) => store::read(f).map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Reads the trash, keeping copies of notes `want`.
    pub fn read(&self, want: &[u32]) -> Res<Snapshot> {
        let Some((head, mut notes)) = self.open()? else {
            return Ok(Snapshot { head: Header::default(), notes: None, numbers: vec![], wanted: vec![] });
        };
        let (mut numbers, mut wanted) = (vec![], vec![]);
        while let Some(note) = notes.next_note()? {
            numbers.push(note.n);
            if want.contains(&note.n) {
                wanted.push((note.n, note.raw.to_owned()));
            }
        }
        wanted.sort_by_key(|w| w.0);
        Ok(Snapshot { head, notes: Some(notes), numbers, wanted })
    }

    /// Rewrites the trash read in `snap`. An entry survives if `keep` says so and it was removed
    /// at or after `cutoff`; then `add` goes on the end. Saved links of dropped entries are
    /// deleted, and the folder goes once it's empty. Returns how many notes were dropped.
    /// The caller holds the owner's lock.
    pub fn update(
        &self,
        snap: Snapshot,
        owner: Option<&Path>,
        now: &str,
        cutoff: &str,
        keep: impl Fn(u32) -> bool,
        add: Incoming,
    ) -> Res<usize> {
        let Snapshot { head, mut notes, numbers, .. } = snap;
        let times = Times::new(&head.removed);
        // Entries from before removal times were recorded count as removed now.
        let when = |n| times.get(n).unwrap_or(now);
        let kept = |n| keep(n) && when(n) >= cutoff;
        let dropped = numbers.iter().filter(|&&n| !kept(n)).count();
        let link_kept = |l: &str| entry(l).is_some_and(|(n, ..)| kept(n));

        // Nothing would change, which is the usual case for a sweep: skip the rewrite and its fsync.
        let settled = owner.is_none()
            && add.is_empty()
            && dropped == 0
            && numbers.iter().all(|&n| times.get(n).is_some())
            && head.links.iter().all(|l| link_kept(l));
        if settled {
            return Ok(0);
        }

        for (n, _, saved) in head.links.iter().filter_map(|l| entry(l)) {
            if !kept(n) {
                let _ = fs::remove_file(self.dir.join(saved)); // already gone is fine
            }
        }
        if add.notes.is_empty() && !numbers.iter().any(|&n| kept(n)) {
            drop(notes);
            let _ = fs::remove_dir_all(&self.dir);
            return Ok(dropped);
        }

        fs::create_dir_all(&self.dir)?;
        let mut out = Rewrite::create(&self.dir.join("trash.nb"))?;
        writeln!(out, "{MAGIC}")?;
        let owner = owner.map(|p| canon(p).unwrap_or_else(|_| p.to_path_buf()).display().to_string());
        if let Some(o) = owner.as_deref().or(head.owner.as_deref()) {
            writeln!(out, "notebook {o}")?;
        }
        for &n in numbers.iter().filter(|&&n| kept(n)) {
            writeln!(out, "removed {n} {}", when(n))?;
        }
        for n in &add.numbers {
            writeln!(out, "removed {n} {now}")?;
        }
        for l in head.links.iter().filter(|l| link_kept(l)).chain(&add.links) {
            writeln!(out, "link {l}")?;
        }
        writeln!(out)?;
        if let Some(r) = notes.as_mut() {
            r.rewind()?;
            while let Some(note) = r.next_note()? {
                if kept(note.n) {
                    out.write_all(note.raw.as_bytes())?;
                }
            }
        }
        out.write_all(add.notes.as_bytes())?;
        drop(notes);
        out.commit()?;
        Ok(dropped)
    }

    /// Where the owning notebook was last seen.
    fn owner(&self) -> Res<Option<PathBuf>> {
        Ok(self.open()?.and_then(|(head, _)| head.owner).map(PathBuf::from))
    }
}

/// Expires old entries in every notebook's trash, so a notebook that's never touched again
/// doesn't keep its trash forever. With `orphans`, also deletes trash whose notebook is gone
/// from its last known path, or has a different uid now. Best effort: trash whose owner is busy
/// is skipped. Returns (orphaned folders deleted, notes expired).
pub fn sweep(env: &Env, orphans: bool) -> (usize, usize) {
    let Ok(entries) = fs::read_dir(&env.cache) else { return (0, 0) };
    let (now, cutoff) = (env.stamp(), env.cutoff());
    let (mut dirs, mut expired) = (0, 0);
    for dir in entries.flatten().map(|e| e.path()).filter(|p| p.join("trash.nb").is_file()) {
        let trash = Trash { dir };
        let Ok(owner) = trash.owner() else { continue };
        let name = trash.dir.file_name().and_then(|n| n.to_str());
        let alive = owner.as_deref().is_some_and(|nb| {
            store::header(nb).and_then(|h| store::uid(&h, nb)).is_ok_and(|uid| Some(uid.as_str()) == name)
        });
        if orphans && owner.is_some() && !alive {
            dirs += fs::remove_dir_all(&trash.dir).is_ok() as usize;
            continue;
        }
        // Don't wait for a busy owner: two sweeps waiting on each other would deadlock.
        let _lock = match owner.as_deref().filter(|_| alive) {
            Some(nb) => match Lock::try_now(&env.cache, nb) {
                Some(lock) => Some(lock),
                None => continue,
            },
            None => None,
        };
        let swept = trash.read(&[]).and_then(|snap| trash.update(snap, None, &now, &cutoff, |_| true, Incoming::default()));
        expired += swept.unwrap_or(0);
    }
    (dirs, expired)
}

/// Splits a link entry into (note number, path relative to the notebook, saved copy).
pub fn entry(l: &str) -> Option<(u32, &str, &str)> {
    let mut it = l.splitn(3, '\t');
    Some((it.next()?.parse().ok()?, it.next()?, it.next()?))
}

/// Removal times, sorted for lookup by note number.
pub struct Times<'a>(Vec<(u32, &'a str)>);

impl<'a> Times<'a> {
    pub fn new(removed: &'a [(u32, String)]) -> Self {
        let mut v: Vec<_> = removed.iter().map(|(n, ts)| (*n, ts.as_str())).collect();
        v.sort_by_key(|r| r.0); // stable, so the first record for a number still wins
        Times(v)
    }

    pub fn get(&self, n: u32) -> Option<&'a str> {
        let i = self.0.partition_point(|r| r.0 < n);
        self.0.get(i).filter(|r| r.0 == n).map(|r| r.1)
    }
}
