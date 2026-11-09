//! notebook (`nb`): one `.nb` file holding many timestamped, tagged markdown notes,
//! plus `.nt` link files that point back into a notebook.
//!
//! `.nb` format:
//! ```text
//! notebook v1
//! id api: API design decisions
//!
//! @@ 1 2026-11-09T12:00:00Z [api] Optional title
//! markdown body
//!
//! ```
//! Body lines starting with `@@` or `\` are stored with one extra leading `\`.

use clap::{Args, Parser, Subcommand};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type Res<T> = Result<T, Box<dyn Error>>;

const NB_MAGIC: &str = "notebook v1";
const NT_MAGIC: &str = "notebook link v1";

#[derive(Parser)]
#[command(name = "notebook", version, about = "A single-file library of tagged markdown notes")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an empty notebook (.nb)
    Make { file: PathBuf },
    /// Add a note. Body comes from --text or stdin. Prints the new note number.
    Add {
        /// Notebook (.nb) or link (.nt)
        file: PathBuf,
        /// Identifier for the note. New identifiers must be defined as NAME:MEANING (repeatable)
        #[arg(long = "id", short = 'i')]
        ids: Vec<String>,
        #[arg(long, short)]
        title: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Edit a note in place. Only the parts you pass are replaced; its timestamp becomes now.
    Edit {
        /// Notebook (.nb) or link (.nt)
        file: PathBuf,
        /// Note number
        n: u32,
        /// Replace the note's identifiers (same rules as add; repeatable)
        #[arg(long = "id", short = 'i')]
        ids: Vec<String>,
        #[arg(long, short)]
        title: Option<String>,
        /// New body; pass - to read it from stdin
        #[arg(long)]
        text: Option<String>,
    },
    /// Remove notes, and delete the .nt links that point at them. Recoverable with `restore`.
    Rm {
        file: PathBuf,
        /// Note numbers
        #[arg(required = true)]
        notes: Vec<u32>,
    },
    /// Bring removed notes, and the links deleted with them, back from the trash
    Restore {
        file: PathBuf,
        /// Note numbers
        #[arg(required = true)]
        notes: Vec<u32>,
    },
    /// List removed notes that can be restored (they expire after NB_TRASH_DAYS, default 30)
    Trash { file: PathBuf },
    /// Permanently delete notes from the trash. Cannot be undone.
    Purge {
        #[arg(required_unless_present = "orphans")]
        file: Option<PathBuf>,
        /// Note numbers to delete from the trash
        #[arg(required_unless_present_any = ["all", "orphans"], conflicts_with = "all")]
        notes: Vec<u32>,
        /// Empty this notebook's whole trash
        #[arg(long)]
        all: bool,
        /// Instead of one notebook: delete trash left behind by notebooks that no longer exist
        #[arg(long, conflicts_with_all = ["file", "notes", "all"])]
        orphans: bool,
    },
    /// Print the identifier dictionary
    Ids { file: PathBuf },
    /// Print note headers only
    List {
        file: PathBuf,
        #[command(flatten)]
        filter: Filter,
    },
    /// Print full notes
    Show {
        file: PathBuf,
        #[command(flatten)]
        filter: Filter,
    },
    /// Create a .nt link to a selection of a notebook
    Link {
        notebook: PathBuf,
        out: PathBuf,
        #[command(flatten)]
        filter: Filter,
    },
}

#[derive(Args, Default, Debug, PartialEq)]
struct Filter {
    /// Note number (repeatable; any match)
    #[arg(long = "note", short = 'n')]
    notes: Vec<u32>,
    /// Identifier the note must carry (repeatable; all must match)
    #[arg(long = "id", short = 'i')]
    ids: Vec<String>,
    /// Earliest timestamp, inclusive; any prefix works (e.g. 2026-11)
    #[arg(long)]
    since: Option<String>,
    /// Latest timestamp, inclusive; any prefix works (e.g. 2026-11-09)
    #[arg(long)]
    until: Option<String>,
}

impl Filter {
    fn matches(&self, n: &Note) -> bool {
        (self.notes.is_empty() || self.notes.contains(&n.n))
            && self.ids.iter().all(|i| n.ids.contains(&i.as_str()))
            && self.since.as_deref().is_none_or(|s| n.ts >= s)
            && self.until.as_deref().is_none_or(|u| &n.ts[..u.len().min(n.ts.len())] <= u)
    }
}

struct Note<'a> {
    n: u32,
    ts: &'a str,
    ids: Vec<&'a str>,
    title: &'a str,
    body: Vec<&'a str>, // raw (still escaped) lines
    start: usize,       // byte offset of the header line
}

impl Note<'_> {
    fn header(&self) -> String {
        let h = format!("{} {} [{}] {}", self.n, self.ts, self.ids.join(","), self.title);
        h.trim_end().to_string()
    }
}

struct Book<'a> {
    ids: Vec<(&'a str, &'a str)>,
    last: u32,            // highest note number ever used, so removed numbers are never reused
    links: Vec<&'a str>,  // registered .nt files, relative to the notebook's folder
    removed: Vec<(u32, &'a str)>, // trash only: when each note was removed
    uid: Option<&'a str>,         // permanent id; names the trash folder so it follows moves
    notebook: Option<&'a str>,    // trash only: last known path of the owning notebook
    notes: Vec<Note<'a>>,
    notes_start: usize,   // byte offset of the first `@@` line
}

impl Book<'_> {
    fn max_n(&self) -> u32 {
        self.notes.iter().map(|x| x.n).max().unwrap_or(0).max(self.last)
    }
    /// Byte range of note i, including its trailing separator line.
    fn span(&self, i: usize, len: usize) -> (usize, usize) {
        (self.notes[i].start, self.notes.get(i + 1).map_or(len, |x| x.start))
    }
}

fn parse(src: &str) -> Res<Book<'_>> {
    let mut book = Book { ids: vec![], last: 0, links: vec![], removed: vec![], uid: None, notebook: None, notes: vec![], notes_start: src.len() };
    let mut off = 0;
    for (i, raw) in src.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        let at = off;
        off += raw.len();
        if i == 0 {
            if line != NB_MAGIC {
                return Err(format!("not a notebook (expected '{NB_MAGIC}' on line 1)").into());
            }
        } else if let Some(h) = line.strip_prefix("@@ ") {
            if book.notes.is_empty() {
                book.notes_start = at;
            }
            let mut note = parse_header(h).ok_or_else(|| format!("bad note header on line {}", i + 1))?;
            note.start = at;
            book.notes.push(note);
        } else if let Some(note) = book.notes.last_mut() {
            note.body.push(line);
        } else if let Some(d) = line.strip_prefix("id ") {
            let (k, v) = d.split_once(": ").ok_or_else(|| format!("bad id line {}", i + 1))?;
            book.ids.push((k, v));
        } else if let Some(v) = line.strip_prefix("last ") {
            book.last = v.parse().map_err(|_| format!("bad last line {}", i + 1))?;
        } else if let Some(v) = line.strip_prefix("link ") {
            book.links.push(v);
        } else if let Some(v) = line.strip_prefix("uid ") {
            // The uid becomes a folder name under the cache: never let it escape that folder.
            if v.is_empty() || v == "." || v == ".." || v.contains(['/', '\\', '\0']) {
                return Err(format!("bad uid on line {}", i + 1).into());
            }
            book.uid = Some(v);
        } else if let Some(v) = line.strip_prefix("notebook ") {
            book.notebook = Some(v);
        } else if let Some(v) = line.strip_prefix("removed ") {
            let bad = || format!("bad removed line {}", i + 1);
            let (n, ts) = v.split_once(' ').ok_or_else(bad)?;
            book.removed.push((n.parse().map_err(|_| bad())?, ts));
        } else if !line.is_empty() {
            return Err(format!("unexpected line {} in id region", i + 1).into());
        }
    }
    for n in &mut book.notes {
        if n.body.last() == Some(&"") {
            n.body.pop(); // separator line
        }
    }
    Ok(book)
}

fn parse_header(h: &str) -> Option<Note<'_>> {
    let mut it = h.splitn(3, ' ');
    let n = it.next()?.parse().ok()?;
    let ts = it.next()?;
    let rest = it.next()?.strip_prefix('[')?;
    let (ids, title) = rest.split_once(']')?;
    let ids = ids.split(',').filter(|s| !s.is_empty()).collect();
    Some(Note { n, ts, ids, title: title.trim(), body: vec![], start: 0 })
}

fn serialize_note(n: u32, ts: &str, ids: &[&str], title: &str, body: &str) -> String {
    let note = Note { n, ts, ids: ids.to_vec(), title, body: vec![], start: 0 };
    let mut s = format!("@@ {}\n", note.header());
    for l in body.lines() {
        if l.starts_with("@@") || l.starts_with('\\') {
            s.push('\\');
        }
        s.push_str(l);
        s.push('\n');
    }
    s.push('\n');
    s
}

fn unescape(l: &str) -> &str {
    l.strip_prefix('\\').unwrap_or(l)
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn now_utc() -> String {
    fmt_ts(now_secs())
}

/// Unix seconds -> RFC 3339 UTC (Hinnant's civil_from_days). Sorts lexicographically.
fn fmt_ts(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}

/// Resolve a path to (notebook path, filter from link). `.nt` files point at a notebook.
fn resolve(path: &Path) -> Res<(PathBuf, Filter)> {
    if path.extension().is_none_or(|e| e != "nt") {
        return Ok((path.to_path_buf(), Filter::default()));
    }
    let src = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = src.lines();
    if lines.next() != Some(NT_MAGIC) {
        return Err(format!("{}: not a notebook link", path.display()).into());
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

fn read(path: &Path) -> Res<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Open and exclusively lock the notebook, retrying if a concurrent writer replaced it.
fn lock(path: &Path) -> Res<File> {
    loop {
        let f = OpenOptions::new().read(true).append(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.lock()?;
        // shortcut: inode check is unix-only; port with file IDs if Windows support is needed.
        use std::os::unix::fs::MetadataExt;
        if f.metadata()?.ino() == fs::metadata(path)?.ino() {
            return Ok(f);
        }
    }
}

/// Body from --text, or stdin when --text is absent or `-`.
fn read_body(text: Option<&str>) -> Res<String> {
    let b = match text {
        Some(t) if t != "-" => t.to_string(),
        _ if io::stdin().is_terminal() => return Err("no body: pass --text or pipe stdin".into()),
        _ => io::read_to_string(io::stdin())?,
    };
    Ok(b.trim_end_matches(['\n', '\r']).to_string())
}

fn check_title(t: Option<&str>) -> Res<()> {
    match t {
        Some(t) if t.contains('\n') => Err("title must be one line".into()),
        _ => Ok(()),
    }
}

/// Validate `--id` args against the dictionary: returns (note ids, newly defined ids).
#[allow(clippy::type_complexity)]
fn check_ids<'a>(book: &Book<'a>, id_args: &'a [String]) -> Res<(Vec<&'a str>, Vec<(&'a str, &'a str)>)> {
    let mut ids: Vec<&str> = vec![];
    let mut new_ids: Vec<(&str, &str)> = vec![];
    for a in id_args {
        let (name, meaning) = match a.split_once(':') {
            Some((n, m)) => (n.trim(), Some(m.trim())),
            None => (a.trim(), None),
        };
        if !valid_id(name) {
            return Err(format!("invalid id '{name}' (use letters, digits, _ - .)").into());
        }
        let known = book.ids.iter().chain(&new_ids).find(|(k, _)| *k == name).map(|(_, v)| *v);
        match (known, meaning) {
            (None, None | Some("")) => {
                return Err(format!("new id '{name}' needs a meaning: --id '{name}:what it means'").into())
            }
            (None, Some(m)) if m.contains('\n') => return Err("id meaning must be one line".into()),
            (None, Some(m)) => new_ids.push((name, m)),
            (Some(k), Some(m)) if k != m => {
                return Err(format!("id '{name}' already means '{k}'; omit the meaning to reuse it").into())
            }
            _ => {}
        }
        if !ids.contains(&name) {
            ids.push(name);
        }
    }
    Ok((ids, new_ids))
}

/// Rewrite the whole notebook atomically (temp file + rename). Caller holds the lock.
fn rewrite<'a>(path: &Path, uid: &str, dict: impl Iterator<Item = (&'a str, &'a str)>, last: u32, links: &[&str], parts: &[&str]) -> Res<()> {
    let mut out = format!("{NB_MAGIC}\nuid {uid}\n");
    for (k, v) in dict {
        out.push_str(&format!("id {k}: {v}\n"));
    }
    if last > 0 {
        out.push_str(&format!("last {last}\n"));
    }
    for l in links {
        out.push_str(&format!("link {l}\n"));
    }
    out.push('\n');
    parts.iter().for_each(|p| out.push_str(p));
    write_atomic(path, &out)
}

fn write_atomic(path: &Path, content: &str) -> Res<()> {
    let tmp = path.with_extension("nb.tmp");
    let mut t = File::create(&tmp)?;
    t.write_all(content.as_bytes())?;
    t.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn add(path: &Path, id_args: &[String], title: Option<&str>, text: Option<&str>) -> Res<u32> {
    check_title(title)?;
    let body = read_body(text)?;

    let mut f = lock(path)?;
    let mut src = String::new();
    (&f).read_to_string(&mut src)?;
    let book = parse(&src)?;
    let (ids, new_ids) = check_ids(&book, id_args)?;

    let n = book.max_n() + 1;
    let note = serialize_note(n, &now_utc(), &ids, title.unwrap_or(""), &body);

    if new_ids.is_empty() {
        f.write_all(note.as_bytes())?;
    } else {
        // New ids go in the top dictionary, so the file must be rewritten.
        let dict = book.ids.iter().chain(&new_ids).copied();
        rewrite(path, &uid_of(&book, path)?, dict, book.last, &book.links, &[&src[book.notes_start..], &note])?;
    }
    Ok(n)
}

fn edit(path: &Path, n: u32, id_args: &[String], title: Option<&str>, text: Option<&str>) -> Res<()> {
    if id_args.is_empty() && title.is_none() && text.is_none() {
        return Err("nothing to change: pass --text, --title or --id".into());
    }
    check_title(title)?;
    let new_body = text.map(|t| read_body(Some(t))).transpose()?;

    let f = lock(path)?;
    let mut src = String::new();
    (&f).read_to_string(&mut src)?;
    let book = parse(&src)?;
    let i = book.notes.iter().position(|x| x.n == n).ok_or_else(|| format!("no note {n}"))?;
    let old = &book.notes[i];
    let (ids, new_ids) = if id_args.is_empty() { (old.ids.clone(), vec![]) } else { check_ids(&book, id_args)? };
    let body = new_body.unwrap_or_else(|| old.body.iter().map(|l| unescape(l)).collect::<Vec<_>>().join("\n"));
    let note = serialize_note(n, &now_utc(), &ids, title.unwrap_or(old.title), &body);

    let (start, end) = book.span(i, src.len());
    let parts = [&src[book.notes_start..start], note.as_str(), &src[end..]];
    rewrite(path, &uid_of(&book, path)?, book.ids.iter().chain(&new_ids).copied(), book.last, &book.links, &parts)
}

fn read_locked(f: &File) -> Res<String> {
    let mut src = String::new();
    { f }.read_to_string(&mut src)?;
    Ok(src)
}

/// The notebook's folder, canonicalized (links are stored relative to it).
fn nb_dir(path: &Path) -> Res<PathBuf> {
    Ok(fs::canonicalize(path)?.parent().ok_or("notebook has no parent folder")?.to_path_buf())
}

/// FNV-1a: stable across Rust versions, unlike DefaultHasher.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// A fresh notebook id: `<file name>-<16 hex>`, unique per creation.
fn new_uid(path: &Path) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seed = format!("{nanos}-{}-{}", std::process::id(), path.display());
    format!("{}-{:016x}", file_name(path), fnv(seed.as_bytes()))
}

/// The notebook's id. Notebooks made before ids existed get the path-based name their trash
/// already uses; it is written into the file on the next rewrite, and stays fixed from then on.
fn uid_of(book: &Book, path: &Path) -> Res<String> {
    if let Some(u) = book.uid {
        return Ok(u.to_string());
    }
    let abs = fs::canonicalize(path)?;
    Ok(format!("{}-{:016x}", file_name(&abs), fnv(abs.as_os_str().as_encoded_bytes())))
}

/// Per-notebook trash folder in the user cache, named by the notebook's uid.
fn trash_dir(cache: &Path, nb: &Path) -> Res<PathBuf> {
    let src = read(nb)?;
    Ok(cache.join(uid_of(&parse(&src)?, nb)?))
}

fn cache_root() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("notebook")
}

fn read_trash(dir: &Path) -> String {
    fs::read_to_string(dir.join("trash.nb")).unwrap_or_else(|_| format!("{NB_MAGIC}\n"))
}

/// Trash entries removed before this time expire. `NB_TRASH_DAYS` sets the age (default 30).
fn trash_cutoff() -> String {
    let days = std::env::var("NB_TRASH_DAYS").ok().and_then(|d| d.parse::<i64>().ok()).unwrap_or(30);
    fmt_ts(now_secs() - days * 86400)
}

#[derive(Default)]
struct TrashAdd<'a> {
    parts: Vec<&'a str>,  // raw note text
    links: Vec<String>,   // "n\trel\tstored"
    removed: Vec<String>, // "n ts"
}

/// Rewrite the trash. Existing entries survive if `keep(n)` and they were removed at or after
/// `cutoff`; the saved links of the others are deleted. Then `add` is appended. An empty trash
/// folder is removed. Returns how many notes were dropped. Caller holds the notebook lock.
fn update_trash(tdir: &Path, nb: Option<&Path>, cutoff: &str, keep: impl Fn(u32) -> bool, add: TrashAdd) -> Res<usize> {
    let tsrc = read_trash(tdir);
    let t = parse(&tsrc)?;
    let now = now_utc();
    // Entries from before removal times were recorded count as removed now.
    let when = |n: u32| t.removed.iter().find(|r| r.0 == n).map_or(now.as_str(), |r| r.1);
    let kept = |n: u32| keep(n) && when(n) >= cutoff;

    let (mut parts, mut removed, mut links, mut dropped) = (vec![], vec![], vec![], 0);
    for i in 0..t.notes.len() {
        let n = t.notes[i].n;
        if kept(n) {
            let (s, e) = t.span(i, tsrc.len());
            parts.push(&tsrc[s..e]);
            removed.push(format!("{n} {}", when(n)));
        } else {
            dropped += 1;
        }
    }
    for l in &t.links {
        let mut it = l.splitn(3, '\t');
        let (Some(n), Some(_), Some(stored)) = (it.next().and_then(|n| n.parse().ok()), it.next(), it.next()) else { continue };
        if kept(n) {
            links.push(l.to_string());
        } else {
            let _ = fs::remove_file(tdir.join(stored)); // already gone is fine
        }
    }
    if parts.is_empty() && add.parts.is_empty() {
        let _ = fs::remove_dir_all(tdir);
        return Ok(dropped);
    }
    parts.extend(add.parts);
    links.extend(add.links);
    removed.extend(add.removed);
    let mut out = format!("{NB_MAGIC}\n");
    // Record the owner's path (refreshed on every update) so orphaned trash can be found.
    let owner = nb.map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).display().to_string());
    if let Some(o) = owner.as_deref().or(t.notebook) {
        out.push_str(&format!("notebook {o}\n"));
    }
    removed.iter().for_each(|r| out.push_str(&format!("removed {r}\n")));
    links.iter().for_each(|l| out.push_str(&format!("link {l}\n")));
    out.push('\n');
    parts.iter().for_each(|p| out.push_str(p));
    fs::create_dir_all(tdir)?;
    write_atomic(&tdir.join("trash.nb"), &out)?;
    Ok(dropped)
}

/// Expire old entries in every notebook's trash, so a notebook that is never touched again
/// doesn't keep its trash forever. With `orphans`, also delete trash whose notebook no longer
/// exists at its last known path (or now has a different uid). Best effort: trash whose owner is
/// busy is skipped. Returns (orphaned folders deleted, notes expired).
fn sweep(cache: &Path, cutoff: &str, orphans: bool) -> (usize, usize) {
    let (mut dirs, mut expired) = (0, 0);
    let Ok(rd) = fs::read_dir(cache) else { return (0, 0) };
    for dir in rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let tsrc = read_trash(&dir);
        let Ok(t) = parse(&tsrc) else { continue };
        let owner = t.notebook.map(PathBuf::from);
        let id = dir.file_name().and_then(|n| n.to_str());
        let alive = owner.as_deref().is_some_and(|nb| {
            read(nb).ok().and_then(|s| parse(&s).ok().and_then(|b| uid_of(&b, nb).ok())).as_deref() == id
        });
        if orphans && owner.is_some() && !alive {
            dirs += fs::remove_dir_all(&dir).is_ok() as usize;
            continue;
        }
        // Lock a live owner without waiting: avoids deadlock with another sweep, and skips our own.
        let _lock = match owner.as_deref().filter(|_| alive) {
            Some(nb) => match File::open(nb).map(|f| f.try_lock().map(|_| f)) {
                Ok(Ok(f)) => Some(f),
                _ => continue,
            },
            None => None,
        };
        expired += update_trash(&dir, None, cutoff, |_| true, TrashAdd::default()).unwrap_or(0);
    }
    (dirs, expired)
}

/// Permanently delete notes from the trash; an empty `ns` deletes everything. Returns how many.
fn purge(path: &Path, ns: &[u32], cache: &Path) -> Res<usize> {
    let _lock = lock(path)?;
    let tdir = trash_dir(cache, path)?;
    let tsrc = read_trash(&tdir);
    let t = parse(&tsrc)?;
    if let Some(n) = ns.iter().find(|n| !t.notes.iter().any(|x| x.n == **n)) {
        return Err(format!("note {n} is not in the trash").into());
    }
    update_trash(&tdir, Some(path), "", |n| !ns.is_empty() && !ns.contains(&n), TrashAdd::default())
}

/// Remove notes. Links that name a removed note, or that would select nothing afterwards, are deleted.
/// Notes and deleted links go to the trash so `restore` can bring them back. Returns deleted link paths.
fn rm(path: &Path, ns: &[u32], cache: &Path) -> Res<Vec<String>> {
    let f = lock(path)?;
    let src = read_locked(&f)?;
    let book = parse(&src)?;
    if let Some(n) = ns.iter().find(|n| !book.notes.iter().any(|x| x.n == **n)) {
        return Err(format!("no note {n}").into());
    }
    let dir = nb_dir(path)?;
    let (mut keep_links, mut gone) = (vec![], vec![]);
    for rel in &book.links {
        let Ok((_, lf)) = resolve(&dir.join(rel)) else { continue }; // link deleted by hand: unregister
        let named = lf.notes.iter().copied().find(|n| ns.contains(n));
        let selected: Vec<u32> = book.notes.iter().filter(|x| lf.matches(x)).map(|x| x.n).collect();
        let emptied = (!selected.is_empty() && selected.iter().all(|n| ns.contains(n))).then(|| selected[0]);
        match named.or(emptied) {
            Some(t) => gone.push((t, *rel)),
            None => keep_links.push(*rel),
        }
    }

    // 1. Save to trash first, so a crash can only duplicate, never lose.
    let tdir = trash_dir(cache, path)?;
    fs::create_dir_all(&tdir)?;
    let now = now_utc();
    let mut add = TrashAdd::default();
    for (i, (t, rel)) in gone.iter().enumerate() {
        let stored = format!("{t}-{i}-{}.nt", now.replace(':', ""));
        fs::copy(dir.join(rel), tdir.join(&stored))?;
        add.links.push(format!("{t}\t{rel}\t{stored}"));
    }
    let mut keep = vec![];
    for i in 0..book.notes.len() {
        let (s, e) = book.span(i, src.len());
        let n = book.notes[i].n;
        if ns.contains(&n) {
            add.parts.push(&src[s..e]);
            add.removed.push(format!("{n} {now}"));
        } else {
            keep.push(&src[s..e]);
        }
    }
    update_trash(&tdir, Some(path), &trash_cutoff(), |_| true, add)?;

    // 2. Rewrite the notebook, 3. delete the links.
    rewrite(path, &uid_of(&book, path)?, book.ids.iter().copied(), book.max_n(), &keep_links, &keep)?;
    for (_, rel) in &gone {
        fs::remove_file(dir.join(rel))?;
    }
    Ok(gone.iter().map(|(_, r)| r.to_string()).collect())
}

/// Bring notes (and the links deleted with them) back from the trash. Returns restored link paths.
fn restore(path: &Path, ns: &[u32], cache: &Path) -> Res<Vec<String>> {
    let f = lock(path)?;
    let src = read_locked(&f)?;
    let book = parse(&src)?;
    let tdir = trash_dir(cache, path)?;
    let tsrc = read_trash(&tdir);
    let tbook = parse(&tsrc)?;
    for n in ns {
        if !tbook.notes.iter().any(|x| x.n == *n) {
            return Err(format!("note {n} is not in the trash").into());
        }
        if book.notes.iter().any(|x| x.n == *n) {
            return Err(format!("note {n} already exists in the notebook").into());
        }
    }
    let mut parts = vec![];
    for i in 0..book.notes.len() {
        let (s, e) = book.span(i, src.len());
        parts.push((book.notes[i].n, &src[s..e]));
    }
    for i in 0..tbook.notes.len() {
        let (s, e) = tbook.span(i, tsrc.len());
        if ns.contains(&tbook.notes[i].n) {
            parts.push((tbook.notes[i].n, &tsrc[s..e]));
        }
    }
    parts.sort_by_key(|p| p.0);
    let mut back = vec![];
    for l in &tbook.links {
        let mut it = l.splitn(3, '\t');
        let (Some(t), Some(rel), Some(stored)) = (it.next(), it.next(), it.next()) else { continue };
        if t.parse().is_ok_and(|t: u32| ns.contains(&t)) {
            back.push((rel, stored));
        }
    }
    let mut links = book.links.clone();
    links.extend(back.iter().map(|(rel, _)| *rel).filter(|r| !book.links.contains(r)));
    let parts: Vec<&str> = parts.into_iter().map(|p| p.1).collect();
    rewrite(path, &uid_of(&book, path)?, book.ids.iter().copied(), book.last, &links, &parts)?;

    let dir = nb_dir(path)?;
    for (rel, stored) in &back {
        let dest = dir.join(rel);
        if dest.exists() {
            eprintln!("warning: {rel} already exists; kept it");
        } else {
            fs::create_dir_all(dest.parent().unwrap_or(&dir))?;
            fs::copy(tdir.join(stored), &dest)?;
        }
    }
    // Drops the restored entries (and their saved link copies) plus anything expired.
    update_trash(&tdir, Some(path), &trash_cutoff(), |n| !ns.contains(&n), TrashAdd::default())?;
    Ok(back.iter().map(|(r, _)| r.to_string()).collect())
}

fn relative(from_dir: &Path, to: &Path) -> PathBuf {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut p: PathBuf = std::iter::repeat_n("..", a.len() - common).collect();
    p.extend(&b[common..]);
    p
}

fn link(nb: &Path, out: &Path, f: &Filter) -> Res<()> {
    let (nb, _) = resolve(nb)?;
    let lk = lock(&nb)?;
    let src = read_locked(&lk)?;
    let book = parse(&src)?; // refuse to link to something that is not a notebook
    let dir = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let rel = relative(&fs::canonicalize(dir)?, &fs::canonicalize(&nb)?);
    let mut s = format!("{NT_MAGIC}\nnotebook: {}\n", rel.display());
    if !f.notes.is_empty() {
        let v: Vec<_> = f.notes.iter().map(u32::to_string).collect();
        s.push_str(&format!("notes: {}\n", v.join(",")));
    }
    if !f.ids.is_empty() {
        s.push_str(&format!("ids: {}\n", f.ids.join(",")));
    }
    if let Some(v) = &f.since {
        s.push_str(&format!("since: {v}\n"));
    }
    if let Some(v) = &f.until {
        s.push_str(&format!("until: {v}\n"));
    }
    s.push_str("\nRead with: nb show <this file>\n");
    OpenOptions::new().write(true).create_new(true).open(out).map_err(|e| format!("{}: {e}", out.display()))?.write_all(s.as_bytes())?;

    // Register the link so `rm` can find it.
    let reg = relative(&nb_dir(&nb)?, &fs::canonicalize(out)?).display().to_string();
    let mut links = book.links.clone();
    links.push(&reg);
    rewrite(&nb, &uid_of(&book, &nb)?, book.ids.iter().copied(), book.last, &links, &[&src[book.notes_start..]])
}

fn run(cli: Cli) -> Res<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    match cli.cmd {
        Cmd::Make { file } => {
            let mut f = OpenOptions::new().write(true).create_new(true).open(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            writeln!(f, "{NB_MAGIC}\nuid {}", new_uid(&file))?;
        }
        Cmd::Add { file, ids, title, text } => {
            let (nb, _) = resolve(&file)?;
            writeln!(out, "{}", add(&nb, &ids, title.as_deref(), text.as_deref())?)?;
        }
        Cmd::Edit { file, n, ids, title, text } => {
            edit(&resolve(&file)?.0, n, &ids, title.as_deref(), text.as_deref())?;
        }
        Cmd::Rm { file, notes } => {
            for l in rm(&resolve(&file)?.0, &notes, &cache_root())? {
                writeln!(out, "deleted link {l}")?;
            }
            sweep(&cache_root(), &trash_cutoff(), false);
        }
        Cmd::Restore { file, notes } => {
            for l in restore(&resolve(&file)?.0, &notes, &cache_root())? {
                writeln!(out, "restored link {l}")?;
            }
            sweep(&cache_root(), &trash_cutoff(), false);
        }
        Cmd::Trash { file } => {
            let tsrc = read_trash(&trash_dir(&cache_root(), &resolve(&file)?.0)?);
            let t = parse(&tsrc)?;
            for n in &t.notes {
                writeln!(out, "{}", n.header())?;
                if let Some((_, ts)) = t.removed.iter().find(|r| r.0 == n.n) {
                    writeln!(out, "  removed {ts}")?;
                }
                for l in t.links.iter().filter_map(|l| l.split_once('\t')).filter(|(k, _)| *k == n.n.to_string()) {
                    writeln!(out, "  link {}", l.1.split('\t').next().unwrap_or(""))?;
                }
            }
        }
        Cmd::Purge { file, notes, all: _, orphans } => {
            let s = |n: usize| if n == 1 { "" } else { "s" };
            if orphans {
                let (dirs, notes) = sweep(&cache_root(), &trash_cutoff(), true);
                writeln!(out, "deleted {dirs} orphaned trash folder{}; {notes} expired note{}", s(dirs), s(notes))?;
            } else {
                // clap guarantees a file, and notes unless --all was passed.
                let n = purge(&resolve(&file.unwrap_or_default())?.0, &notes, &cache_root())?;
                writeln!(out, "purged {n} note{}", s(n))?;
                sweep(&cache_root(), &trash_cutoff(), false);
            }
        }
        Cmd::Ids { file } => {
            let src = read(&resolve(&file)?.0)?;
            for (k, v) in parse(&src)?.ids {
                writeln!(out, "{k}: {v}")?;
            }
        }
        Cmd::List { ref file, ref filter } | Cmd::Show { ref file, ref filter } => {
            let show = matches!(cli.cmd, Cmd::Show { .. });
            let (nb, lf) = resolve(file)?;
            let src = read(&nb)?;
            for n in parse(&src)?.notes.iter().filter(|n| lf.matches(n) && filter.matches(n)) {
                if show {
                    writeln!(out, "@@ {}", n.header())?;
                    for l in &n.body {
                        writeln!(out, "{}", unescape(l))?;
                    }
                    writeln!(out)?;
                } else {
                    writeln!(out, "{}", n.header())?;
                }
            }
        }
        Cmd::Link { notebook, out: o, filter } => link(&notebook, &o, &filter)?,
    }
    out.flush()?;
    Ok(())
}

pub fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(fmt_ts(0), "1970-01-01T00:00:00Z");
        assert_eq!(fmt_ts(951868800), "2000-03-01T00:00:00Z");
        assert_eq!(fmt_ts(1762689600), "2025-11-09T12:00:00Z");
    }

    #[test]
    fn roundtrip_with_escapes() {
        let body = "line\n@@ 9 fake [x] header\n\\back\n\n# md";
        let src = format!("{NB_MAGIC}\nid a: thing A\n\n{}", serialize_note(1, "2026-11-09T00:00:00Z", &["a"], "T", body));
        let b = parse(&src).unwrap();
        assert_eq!(b.ids, vec![("a", "thing A")]);
        assert_eq!(b.notes.len(), 1);
        let n = &b.notes[0];
        assert_eq!((n.n, n.title, n.ids.clone()), (1, "T", vec!["a"]));
        let got: Vec<_> = n.body.iter().map(|l| unescape(l)).collect();
        assert_eq!(got.join("\n"), body);
    }

    #[test]
    fn filters() {
        let n = parse_header("3 2026-11-09T10:00:00Z [a,b] t").unwrap();
        let f = |notes: Vec<u32>, ids: &[&str], since: Option<&str>, until: Option<&str>| Filter {
            notes,
            ids: ids.iter().map(|s| s.to_string()).collect(),
            since: since.map(Into::into),
            until: until.map(Into::into),
        };
        assert!(f(vec![], &["a", "b"], Some("2026-11"), Some("2026-11-09")).matches(&n));
        assert!(!f(vec![], &["a", "c"], None, None).matches(&n));
        assert!(!f(vec![4], &[], None, None).matches(&n));
        assert!(!f(vec![], &[], Some("2026-11-10"), None).matches(&n));
        assert!(!f(vec![], &[], None, Some("2026-11-08")).matches(&n));
    }

    #[test]
    fn edit_in_place() {
        let p = std::env::temp_dir().join(format!("nb-edit-test-{}.nb", std::process::id()));
        fs::write(&p, format!("{NB_MAGIC}\n")).unwrap();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        add(&p, &s(&["a:thing A"]), Some("one"), Some("first")).unwrap();
        add(&p, &s(&["a"]), Some("two"), Some("@@ escaped\nsecond")).unwrap();
        add(&p, &s(&[]), Some("three"), Some("third")).unwrap();

        edit(&p, 2, &[], Some("two v2"), None).unwrap(); // title only: body must survive with escapes
        edit(&p, 3, &s(&["b:thing B"]), None, Some("third v2")).unwrap();
        assert!(edit(&p, 9, &[], Some("x"), None).is_err());
        assert!(edit(&p, 1, &[], None, None).is_err());

        let src = fs::read_to_string(&p).unwrap();
        let b = parse(&src).unwrap();
        assert_eq!(b.ids, vec![("a", "thing A"), ("b", "thing B")]);
        let got: Vec<_> = b.notes.iter().map(|n| (n.n, n.title, n.ids.join(","), n.body.iter().map(|l| unescape(l)).collect::<Vec<_>>().join("\n"))).collect();
        assert_eq!(
            got,
            vec![
                (1, "one", "a".into(), "first".into()),
                (2, "two v2", "a".into(), "@@ escaped\nsecond".into()),
                (3, "three", "b".into(), "third v2".into()),
            ]
        );
        fs::remove_file(&p).unwrap();
    }

    #[test]
    fn rm_and_restore() {
        let root = std::env::temp_dir().join(format!("nb-rm-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (cache, sub) = (root.join("cache"), root.join("sub"));
        fs::create_dir_all(&sub).unwrap();
        let p = root.join("t.nb");
        fs::write(&p, format!("{NB_MAGIC}\n")).unwrap();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        add(&p, &s(&["a:A", "b:B"]), Some("one"), Some("1")).unwrap();
        add(&p, &s(&["a", "b"]), Some("two"), Some("2")).unwrap();
        add(&p, &s(&["c:C"]), Some("three"), Some("3")).unwrap();
        let f = |notes: Vec<u32>, ids: &[&str]| Filter { notes, ids: s(ids), ..Default::default() };
        link(&p, &sub.join("n2.nt"), &f(vec![2], &[])).unwrap(); // names note 2: deleted
        link(&p, &sub.join("a.nt"), &f(vec![], &["a"])).unwrap(); // group keeps note 1: kept
        link(&p, &root.join("c.nt"), &f(vec![], &["c"])).unwrap(); // only note 3: deleted when 3 goes

        let mut gone = rm(&p, &[2, 3], &cache).unwrap();
        gone.sort();
        assert_eq!(gone, vec!["c.nt", "sub/n2.nt"]);
        assert!(!sub.join("n2.nt").exists() && !root.join("c.nt").exists() && sub.join("a.nt").exists());
        assert!(rm(&p, &[2], &cache).is_err());
        assert_eq!(add(&p, &[], Some("four"), Some("4")).unwrap(), 4); // 2 and 3 are never reused

        let numbers = |p: &Path| parse(&fs::read_to_string(p).unwrap()).unwrap().notes.iter().map(|n| n.n).collect::<Vec<_>>();
        assert_eq!(numbers(&p), vec![1, 4]);
        assert_eq!(numbers(&trash_dir(&cache, &p).unwrap().join("trash.nb")), vec![2, 3]);

        assert_eq!(restore(&p, &[2], &cache).unwrap(), vec!["sub/n2.nt"]);
        assert_eq!(numbers(&p), vec![1, 2, 4]);
        assert_eq!(fs::read_to_string(sub.join("n2.nt")).unwrap().lines().nth(1), Some("notebook: ../t.nb"));
        assert!(restore(&p, &[2], &cache).is_err());
        assert_eq!(numbers(&trash_dir(&cache, &p).unwrap().join("trash.nb")), vec![3]);
        let src = fs::read_to_string(&p).unwrap();
        let mut links = parse(&src).unwrap().links;
        links.sort();
        assert_eq!(links, vec!["sub/a.nt", "sub/n2.nt"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn purge_and_expiry() {
        let root = std::env::temp_dir().join(format!("nb-purge-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let (p, cache) = (root.join("t.nb"), root.join("cache"));
        fs::write(&p, format!("{NB_MAGIC}\n")).unwrap();
        for t in ["1", "2", "3", "4"] {
            add(&p, &[], Some(t), Some(t)).unwrap();
        }
        let f = Filter { notes: vec![1], ..Default::default() };
        link(&p, &root.join("one.nt"), &f).unwrap();
        rm(&p, &[1, 2, 3], &cache).unwrap();

        let tdir = trash_dir(&cache, &p).unwrap();
        let in_trash = || parse(&read_trash(&tdir)).unwrap().notes.iter().map(|n| n.n).collect::<Vec<_>>();
        let saved_links = || fs::read_dir(&tdir).map_or(0, |d| d.filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "nt")).count());
        assert_eq!((in_trash(), saved_links()), (vec![1, 2, 3], 1));
        assert_eq!(parse(&read_trash(&tdir)).unwrap().removed.len(), 3);

        assert_eq!(purge(&p, &[2], &cache).unwrap(), 1);
        assert!(purge(&p, &[2], &cache).is_err());
        assert_eq!(in_trash(), vec![1, 3]);

        // Nothing is older than year 2000; everything is older than year 9999.
        assert_eq!(update_trash(&tdir, None, "2000", |_| true, TrashAdd::default()).unwrap(), 0);
        assert_eq!(in_trash(), vec![1, 3]);
        assert_eq!(update_trash(&tdir, None, "9999", |_| true, TrashAdd::default()).unwrap(), 2);
        assert!(!tdir.exists()); // expired entries and the saved link copy are gone

        rm(&p, &[4], &cache).unwrap();
        assert_eq!(purge(&p, &[], &cache).unwrap(), 1);
        assert!(!tdir.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn trash_follows_moves_and_orphans_are_swept() {
        let root = std::env::temp_dir().join(format!("nb-move-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("moved")).unwrap();
        let (p, cache) = (root.join("t.nb"), root.join("cache"));
        fs::write(&p, format!("{NB_MAGIC}\n")).unwrap(); // made before uids existed
        for t in ["1", "2"] {
            add(&p, &[], Some(t), Some(t)).unwrap();
        }
        let legacy = trash_dir(&cache, &p).unwrap();
        rm(&p, &[1], &cache).unwrap();
        assert!(fs::read_to_string(&p).unwrap().lines().nth(1).unwrap().starts_with("uid t.nb-")); // uid now pinned

        let q = root.join("moved/renamed.nb");
        fs::rename(&p, &q).unwrap();
        assert_eq!(trash_dir(&cache, &q).unwrap(), legacy); // same trash after move + rename
        restore(&q, &[1], &cache).unwrap();
        let src = fs::read_to_string(&q).unwrap();
        assert_eq!(parse(&src).unwrap().notes.len(), 2);

        // Owner path is recorded; deleting the notebook makes its trash an orphan.
        rm(&q, &[2], &cache).unwrap();
        let rec = read_trash(&legacy);
        assert_eq!(parse(&rec).unwrap().notebook, Some(fs::canonicalize(&q).unwrap().to_str().unwrap()));
        assert_eq!(sweep(&cache, "", false), (0, 0)); // owner alive: nothing touched
        fs::remove_file(&q).unwrap();
        assert_eq!(sweep(&cache, "", true), (1, 0));
        assert!(!legacy.exists());

        assert!(parse(&format!("{NB_MAGIC}\nuid ../escape\n")).is_err());
        assert!(parse(&format!("{NB_MAGIC}\nuid ..\n")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/a/b/c"), Path::new("/a/x/n.nb")), PathBuf::from("../../x/n.nb"));
        assert_eq!(relative(Path::new("/a"), Path::new("/a/n.nb")), PathBuf::from("n.nb"));
    }
}
