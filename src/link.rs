//! `.nt` link files: a relative path to a notebook plus a saved selection.
//!
//! ```text
//! notebook link v1
//! notebook: ../backend.nb
//! ids: auth
//!
//! Read with: nb show <this file>
//! ```

use crate::Res;
use crate::filter::Filter;
use crate::paths::at;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAGIC: &str = "notebook link v1";

/// The notebook `path` stands for, and the selection it carries. A notebook stands for itself
/// and selects everything.
pub fn resolve(path: &Path) -> Res<(PathBuf, Filter)> {
    if path.extension().is_none_or(|e| e != "nt") {
        return Ok((path.to_path_buf(), Filter::default()));
    }
    let src = fs::read_to_string(path).map_err(|e| at(path, e))?;
    let mut lines = src.lines();
    if lines.next() != Some(MAGIC) {
        return Err(at(path, "not a notebook link").into());
    }
    let (mut nb, mut f) = (None, Filter::default());
    for l in lines {
        let Some((k, v)) = l.split_once(": ") else { continue };
        match k {
            "notebook" => nb = Some(path.parent().unwrap_or(Path::new("")).join(v)),
            "notes" => f.notes = v.split(',').map(str::parse).collect::<Result<_, _>>()?,
            "ids" => f.ids = v.split(',').map(String::from).collect(),
            "since" => f.since = Some(v.into()),
            "until" => f.until = Some(v.into()),
            _ => {}
        }
    }
    Ok((nb.ok_or("link has no notebook line")?, f))
}

/// Creates the link file `out`, pointing at `target` (relative to `out`'s folder). Fails if
/// `out` already exists.
pub fn create(out: &Path, target: &str, f: &Filter) -> Res<()> {
    let mut s = format!("{MAGIC}\nnotebook: {target}\n");
    if let Some((first, rest)) = f.notes.split_first() {
        let _ = write!(s, "notes: {first}");
        for n in rest {
            let _ = write!(s, ",{n}");
        }
        s.push('\n');
    }
    if !f.ids.is_empty() {
        let _ = writeln!(s, "ids: {}", f.ids.join(","));
    }
    if let Some(v) = &f.since {
        let _ = writeln!(s, "since: {v}");
    }
    if let Some(v) = &f.until {
        let _ = writeln!(s, "until: {v}");
    }
    s.push_str("\nRead with: nb show <this file>\n");
    let mut file = OpenOptions::new().write(true).create_new(true).open(out).map_err(|e| at(out, e))?;
    Ok(file.write_all(s.as_bytes())?)
}
