//! Commands that create or change notes: `make`, `add`, `edit` and `link`.

use crate::Res;
use crate::env::Env;
use crate::filter::Filter;
use crate::format::{self, Header, MAGIC};
use crate::link;
use crate::paths::{at, canon, folder, relative, slash};
use crate::store::{self, Lock, Rewrite};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Creates an empty notebook. Fails if the file exists.
pub fn make(path: &Path) -> Res<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| at(path, e))?;
    writeln!(f, "{MAGIC}\nuid {}", store::new_uid(path))?;
    Ok(())
}

/// Adds a note and returns its number.
pub fn add(env: &Env, path: &Path, id_args: &[String], title: Option<&str>, body: &str) -> Res<u32> {
    check_title(title)?;
    let _lock = Lock::wait(&env.cache, path)?;
    let (head, mut notes) = store::open(path)?;
    let (ids, new_ids) = check_ids(&head, id_args)?;
    let (ts, title) = (env.stamp(), title.unwrap_or(""));
    let mut last = head.last;

    if new_ids.is_empty() {
        // Append. The notes are still read, to find the highest number in use.
        while let Some(note) = notes.next_note()? {
            last = last.max(note.n);
        }
        drop(notes);
        let mut note = Vec::new();
        format::write_note(&mut note, last + 1, &ts, &ids, title, body)?;
        OpenOptions::new().append(true).open(path)?.write_all(&note)?;
        return Ok(last + 1);
    }

    // New identifiers go in the dictionary at the top, so the whole file is rewritten.
    let mut out = Rewrite::create(path)?;
    format::write_header(&mut out, &store::uid(&head, path)?, head.ids().chain(new_ids), head.last, head.links())?;
    while let Some(note) = notes.next_note()? {
        last = last.max(note.n);
        out.write_all(note.raw.as_bytes())?;
    }
    drop(notes);
    format::write_note(&mut out, last + 1, &ts, &ids, title, body)?;
    out.commit()?;
    Ok(last + 1)
}

/// Changes note `n` in place. Only what's passed changes, and its timestamp becomes now.
pub fn edit(env: &Env, path: &Path, n: u32, id_args: &[String], title: Option<&str>, body: Option<&str>) -> Res<()> {
    if id_args.is_empty() && title.is_none() && body.is_none() {
        return Err("nothing to change: pass --text, --title or --id".into());
    }
    check_title(title)?;
    let _lock = Lock::wait(&env.cache, path)?;
    let (head, mut notes) = store::open(path)?;
    let no_note = || format!("no note {n}").into();
    let checked = match (!id_args.is_empty()).then(|| check_ids(&head, id_args)).transpose() {
        Ok(checked) => checked,
        // A missing note matters more than a bad identifier, so it's reported first.
        Err(e) => {
            while let Some(note) = notes.next_note()? {
                if note.n == n {
                    return Err(e);
                }
            }
            return Err(no_note());
        }
    };

    let mut out = Rewrite::create(path)?;
    let new_ids = checked.as_ref().map_or(&[][..], |(_, new)| new.as_slice());
    format::write_header(&mut out, &store::uid(&head, path)?, head.ids().chain(new_ids.iter().copied()), head.last, head.links())?;
    let mut found = false;
    while let Some(note) = notes.next_note()? {
        if note.n != n || found {
            out.write_all(note.raw.as_bytes())?;
            continue;
        }
        found = true;
        let (old_ids, old_body);
        let ids = match &checked {
            Some((ids, _)) => ids.as_slice(),
            None => {
                old_ids = note.ids().collect::<Vec<_>>();
                &old_ids
            }
        };
        let body = match body {
            Some(b) => b,
            None => {
                old_body = note.body();
                &old_body
            }
        };
        format::write_note(&mut out, n, &env.stamp(), ids, title.unwrap_or(note.title), body)?;
    }
    drop(notes);
    if !found {
        return Err(no_note());
    }
    out.commit()
}

/// Creates a `.nt` link to a selection of a notebook and registers it there, so `rm` can find it.
pub fn link(env: &Env, notebook: &Path, out: &Path, f: &Filter) -> Res<()> {
    let (nb, _) = link::resolve(notebook)?;
    let _lock = Lock::wait(&env.cache, &nb)?;
    let (head, mut notes) = store::open(&nb)?;
    let out_dir = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let (out_dir, abs) = (canon(out_dir)?, canon(&nb)?);
    let name = out.file_name().ok_or_else(|| at(out, "not a file name"))?;
    let target = slash(&relative(&out_dir, &abs));
    let registered = slash(&relative(folder(&abs)?, &out_dir.join(name)));

    // Copy the notebook with the link registered, and swap the copy in only once the link exists.
    let mut copy = Rewrite::create(&nb)?;
    let links = head.links().chain([registered.as_str()]);
    format::write_header(&mut copy, &store::uid(&head, &nb)?, head.ids(), head.last, links)?;
    while let Some(note) = notes.next_note()? {
        copy.write_all(note.raw.as_bytes())?;
    }
    drop(notes);
    link::create(out, &target, f)?;
    copy.commit()
}

fn check_title(t: Option<&str>) -> Res<()> {
    match t {
        Some(t) if t.contains('\n') => Err("title must be one line".into()),
        _ => Ok(()),
    }
}

/// A note's identifiers, and those among them being defined for the first time.
type Ids<'a> = (Vec<&'a str>, Vec<(&'a str, &'a str)>);

/// Checks `--id` arguments against the dictionary. Each is `name`, which must already be
/// defined, or `name:meaning`, which defines it (or must match the meaning it already has).
fn check_ids<'a>(head: &'a Header, args: &'a [String]) -> Res<Ids<'a>> {
    let (mut ids, mut new) = (vec![], vec![]);
    for a in args {
        let (name, meaning) = match a.split_once(':') {
            Some((n, m)) => (n.trim(), Some(m.trim())),
            None => (a.trim(), None),
        };
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) {
            return Err(format!("invalid id '{name}' (use letters, digits, _ - .)").into());
        }
        let known = head.ids().chain(new.iter().copied()).find(|(k, _)| *k == name).map(|(_, v)| v);
        match (known, meaning) {
            (None, None | Some("")) => {
                return Err(format!("new id '{name}' needs a meaning: --id '{name}:what it means'").into());
            }
            (None, Some(m)) if m.contains('\n') => return Err("id meaning must be one line".into()),
            (None, Some(m)) => new.push((name, m)),
            (Some(k), Some(m)) if k != m => {
                return Err(format!("id '{name}' already means '{k}'; omit the meaning to reuse it").into());
            }
            _ => {}
        }
        if !ids.contains(&name) {
            ids.push(name);
        }
    }
    Ok((ids, new))
}
