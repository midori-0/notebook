//! The read-only commands: `ids`, `list`, `show` and `trash`.

use crate::Res;
use crate::env::Env;
use crate::filter::Filter;
use crate::link::resolve;
use crate::store;
use crate::trash::{Times, Trash, entry};
use std::io::Write;
use std::path::Path;

pub enum View {
    Headers,
    Full,
}

/// Prints the identifier dictionary. Only the header is read.
pub fn ids(path: &Path, out: &mut impl Write) -> Res<()> {
    for (k, v) in store::header(&resolve(path)?.0)?.ids() {
        writeln!(out, "{k}: {v}")?;
    }
    Ok(())
}

/// Prints the notes `path` selects (all of them for a notebook) that also match `filter`.
pub fn print(path: &Path, filter: &Filter, view: View, out: &mut impl Write) -> Res<()> {
    let (nb, selection) = resolve(path)?;
    let (_, mut notes) = store::open(&nb)?;
    while let Some(note) = notes.next_note()? {
        if !(selection.matches(&note) && filter.matches(&note)) {
            continue;
        }
        match view {
            View::Headers => writeln!(out, "{note}")?,
            View::Full => {
                writeln!(out, "@@ {note}")?;
                for line in note.lines() {
                    out.write_all(line.as_bytes())?;
                    out.write_all(b"\n")?;
                }
                out.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

/// Prints each note in the trash with when it was removed and the links deleted with it.
pub fn trash(env: &Env, path: &Path, out: &mut impl Write) -> Res<()> {
    let nb = resolve(path)?.0;
    let uid = store::uid(&store::header(&nb)?, &nb)?;
    let Some((head, mut notes)) = Trash::of(&env.cache, &uid).open()? else { return Ok(()) };
    let times = Times::new(&head.removed);
    while let Some(note) = notes.next_note()? {
        writeln!(out, "{note}")?;
        if let Some(ts) = times.get(note.n) {
            writeln!(out, "  removed {ts}")?;
        }
        for (_, rel, _) in head.links.iter().filter_map(|l| entry(l)).filter(|e| e.0 == note.n) {
            writeln!(out, "  link {rel}")?;
        }
    }
    Ok(())
}
