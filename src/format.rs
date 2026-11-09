//! The `.nb` text format.
//!
//! ```text
//! notebook v1
//! uid biology.nb-64c6f45652a04287
//! id cells: Single-celled organisms
//! last 4
//! link docs/cells.nt
//!
//! @@ 1 2026-11-09T12:56:12Z [cells] Amoeba movement
//! Amoebas move by extending pseudopods.
//!
//! ```
//!
//! Everything above the first `@@ ` line is the header. A note is its `@@ ` line, a markdown body
//! and one blank separator line. Body lines starting with `@@` or `\` are stored with an extra
//! `\` in front, so no body line can pass for a note header.

use crate::Res;
use std::fmt::{self, Write as _};
use std::io::{self, BufRead, Seek, SeekFrom, Write};

pub const MAGIC: &str = "notebook v1";

/// Everything above the first note. Trash files use the same format plus two kinds of line.
#[derive(Default)]
pub struct Header {
    pub uid: Option<String>,
    pub ids: Vec<(String, String)>,
    /// The highest note number ever used, so a removed note's number is never handed out again.
    pub last: u32,
    /// Registered `.nt` links, relative to the notebook's folder.
    pub links: Vec<String>,
    /// Trash only: where the owning notebook was last seen.
    pub owner: Option<String>,
    /// Trash only: when each note was removed.
    pub removed: Vec<(u32, String)>,
}

impl Header {
    pub fn ids(&self) -> impl Iterator<Item = (&str, &str)> + Clone {
        self.ids.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn links(&self) -> impl Iterator<Item = &str> + Clone {
        self.links.iter().map(String::as_str)
    }

    fn parse_line(&mut self, line: &str, no: usize) -> Res<()> {
        let bad = |kind: &str| format!("bad {kind} line {no}");
        if let Some(d) = line.strip_prefix("id ") {
            let (k, v) = d.split_once(": ").ok_or_else(|| bad("id"))?;
            self.ids.push((k.into(), v.into()));
        } else if let Some(v) = line.strip_prefix("last ") {
            self.last = v.parse().map_err(|_| bad("last"))?;
        } else if let Some(v) = line.strip_prefix("link ") {
            self.links.push(v.into());
        } else if let Some(v) = line.strip_prefix("uid ") {
            // The uid names a folder in the cache, so it must not be able to climb out of it.
            if v.is_empty() || v == "." || v == ".." || v.contains(['/', '\\', ':', '\0']) {
                return Err(format!("bad uid on line {no}").into());
            }
            self.uid = Some(v.into());
        } else if let Some(v) = line.strip_prefix("notebook ") {
            self.owner = Some(v.into());
        } else if let Some(v) = line.strip_prefix("removed ") {
            let (n, ts) = v.split_once(' ').ok_or_else(|| bad("removed"))?;
            self.removed.push((n.parse().map_err(|_| bad("removed"))?, ts.into()));
        } else if !line.is_empty() {
            return Err(format!("unexpected line {no} in id region").into());
        }
        Ok(())
    }
}

/// Writes a notebook header, ending with the blank line before the notes.
pub fn write_header<'a, 'b>(
    w: &mut impl Write,
    uid: &str,
    ids: impl IntoIterator<Item = (&'a str, &'a str)>,
    last: u32,
    links: impl IntoIterator<Item = &'b str>,
) -> io::Result<()> {
    writeln!(w, "{MAGIC}\nuid {uid}")?;
    for (k, v) in ids {
        writeln!(w, "id {k}: {v}")?;
    }
    if last > 0 {
        writeln!(w, "last {last}")?;
    }
    for l in links {
        writeln!(w, "link {l}")?;
    }
    writeln!(w)
}

/// Writes a new note: header line, escaped body, separator.
pub fn write_note(w: &mut impl Write, n: u32, ts: &str, ids: &[&str], title: &str, body: &str) -> io::Result<()> {
    writeln!(w, "@@ {}", Head { n, ts, ids: ids.iter().copied(), title })?;
    for line in body.lines() {
        if line.starts_with("@@") || line.starts_with('\\') {
            w.write_all(b"\\")?;
        }
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
    }
    w.write_all(b"\n")
}

/// A note header without its `@@ ` marker: `<n> <timestamp> [<ids>] <title>`.
struct Head<'a, I> {
    n: u32,
    ts: &'a str,
    ids: I,
    title: &'a str,
}

impl<'a, I: Iterator<Item = &'a str> + Clone> fmt::Display for Head<'a, I> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} [", self.n, self.ts)?;
        for (i, id) in self.ids.clone().enumerate() {
            if i > 0 {
                f.write_char(',')?;
            }
            f.write_str(id)?;
        }
        f.write_char(']')?;
        match self.title.trim_end() {
            "" => Ok(()),
            t => write!(f, " {t}"),
        }
    }
}

/// One note, borrowed from the reader's buffer. `Display` gives its header line minus `@@ `.
pub struct Note<'a> {
    pub n: u32,
    pub ts: &'a str,
    pub title: &'a str,
    /// The note exactly as stored, header line through separator.
    pub raw: &'a str,
    ids: &'a str,
    body: &'a str,
}

impl<'a> Note<'a> {
    pub fn parse(raw: &'a str) -> Option<Self> {
        let (head, rest) = raw.split_once('\n').unwrap_or((raw, ""));
        let mut it = head.trim_end_matches('\r').strip_prefix("@@ ")?.splitn(3, ' ');
        let n = it.next()?.parse().ok()?;
        let ts = it.next()?;
        let (ids, title) = it.next()?.strip_prefix('[')?.split_once(']')?;
        Some(Note { n, ts, title: title.trim(), raw, ids, body: without_separator(rest) })
    }

    pub fn ids(&self) -> impl Iterator<Item = &'a str> + Clone {
        self.ids.split(',').filter(|s| !s.is_empty())
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.ids().any(|x| x == id)
    }

    /// Body lines, unescaped.
    pub fn lines(&self) -> impl Iterator<Item = &'a str> {
        self.body.split_inclusive('\n').map(|l| unescape(l.trim_end_matches(['\n', '\r'])))
    }

    /// The body as one string, unescaped.
    pub fn body(&self) -> String {
        let mut s = String::with_capacity(self.body.len());
        for (i, l) in self.lines().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(l);
        }
        s
    }
}

impl fmt::Display for Note<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&Head { n: self.n, ts: self.ts, ids: self.ids(), title: self.title }, f)
    }
}

fn unescape(line: &str) -> &str {
    line.strip_prefix('\\').unwrap_or(line)
}

/// `rest` without the blank line that ends a note, if it has one.
fn without_separator(rest: &str) -> &str {
    let last = rest.strip_suffix('\n').unwrap_or(rest).rfind('\n').map_or(0, |i| i + 1);
    if !rest.is_empty() && rest[last..].trim_end_matches(['\n', '\r']).is_empty() { &rest[..last] } else { rest }
}

/// Reads a notebook one note at a time. Only about one note is held in memory, so a large
/// notebook costs no more to read than a small one.
pub struct Reader<R> {
    src: R,
    buf: Vec<u8>,
    /// The note returned last is `buf[start..next]`; the following one starts at `next`.
    start: usize,
    next: usize,
    /// `buf` has been searched for note boundaries up to here.
    scanned: usize,
    eof: bool,
    /// Line number of the note at `next`, for errors.
    line: usize,
    lines: usize,
    /// Size of the header in bytes and lines, so `rewind` can skip straight to the notes.
    head_bytes: u64,
    head_lines: usize,
}

impl<R: BufRead> Reader<R> {
    /// Reads the header and stops at the first note.
    pub fn open(mut src: R) -> Res<(Header, Self)> {
        let not_a_notebook = || format!("not a notebook (expected '{MAGIC}' on line 1)");
        let mut head = Header::default();
        let mut buf = Vec::new();
        let (mut lines, mut bytes) = (0, 0);
        loop {
            buf.clear();
            let len = src.read_until(b'\n', &mut buf)?;
            if len == 0 || (lines > 0 && buf.starts_with(b"@@ ")) {
                break;
            }
            lines += 1;
            bytes += len as u64;
            let line = utf8(&buf, lines)?.trim_end_matches(['\n', '\r']);
            if lines > 1 {
                head.parse_line(line, lines)?;
            } else if line != MAGIC {
                return Err(not_a_notebook().into());
            }
        }
        if lines == 0 {
            return Err(not_a_notebook().into());
        }
        // `buf` now holds the first note's header line, or nothing if there are no notes.
        let reader = Reader {
            src,
            buf,
            start: 0,
            next: 0,
            scanned: 0,
            eof: false,
            line: lines + 1,
            lines,
            head_bytes: bytes,
            head_lines: lines,
        };
        Ok((head, reader))
    }

    /// The next note, or `None` after the last.
    pub fn next_note(&mut self) -> Res<Option<Note<'_>>> {
        self.start = self.next;
        if self.start == self.buf.len() && !self.fill()? {
            return Ok(None);
        }
        let line = self.line;
        // The note runs until the next line that starts with `@@ `.
        let end = loop {
            match newline(&self.buf[self.scanned..]) {
                Some(i) => {
                    let at = self.scanned + i + 1;
                    // Deciding needs the next line's first three bytes. (`fill` can move
                    // `buf`, so search again after it.)
                    if self.buf.len() < at + 3 && !self.eof {
                        self.fill()?;
                        continue;
                    }
                    self.scanned = at;
                    self.lines += 1;
                    if self.buf[at..].starts_with(b"@@ ") {
                        break at;
                    }
                }
                None => {
                    self.scanned = self.buf.len();
                    if !self.fill()? {
                        break self.buf.len();
                    }
                }
            }
        };
        (self.next, self.line) = (end, self.lines + 1);
        let text = utf8(&self.buf[self.start..end], line)?;
        Note::parse(text).map(Some).ok_or_else(|| format!("bad note header on line {line}").into())
    }

    /// Appends the next chunk of input to `buf`; false at the end. Notes already returned are
    /// dropped first, so `buf` never holds much more than one note.
    fn fill(&mut self) -> io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.scanned -= self.start;
            self.next -= self.start;
            self.start = 0;
        }
        let chunk = self.src.fill_buf()?;
        let n = chunk.len();
        self.buf.extend_from_slice(chunk);
        self.src.consume(n);
        self.eof = n == 0;
        Ok(n > 0)
    }
}

impl<R: BufRead + Seek> Reader<R> {
    /// Goes back to the first note for another pass.
    pub fn rewind(&mut self) -> Res<()> {
        self.src.seek(SeekFrom::Start(self.head_bytes))?;
        self.buf.clear();
        (self.start, self.next, self.scanned, self.eof) = (0, 0, 0, false);
        (self.lines, self.line) = (self.head_lines, self.head_lines + 1);
        Ok(())
    }
}

/// Index of the first `\n` in `h`, checking eight bytes at a time. (Three times faster than a
/// byte loop, and std's memchr isn't public.)
fn newline(h: &[u8]) -> Option<usize> {
    const ONES: u64 = u64::from_ne_bytes([1; 8]);
    let (words, tail) = h.as_chunks::<8>();
    for (i, w) in words.iter().enumerate() {
        let x = u64::from_le_bytes(*w) ^ (ONES * b'\n' as u64);
        // The lowest set bit marks the first zero byte of `x`, which is the first newline.
        let zero = x.wrapping_sub(ONES) & !x & (ONES << 7);
        if zero != 0 {
            return Some(i * 8 + zero.trailing_zeros() as usize / 8);
        }
    }
    tail.iter().position(|&b| b == b'\n').map(|p| words.len() * 8 + p)
}

/// `bytes` as text; `line` is the line they start on, for the error.
fn utf8(bytes: &[u8], line: usize) -> Res<&str> {
    std::str::from_utf8(bytes).map_err(|e| {
        let at = line + bytes[..e.valid_up_to()].iter().filter(|&&b| b == b'\n').count();
        format!("invalid UTF-8 on line {at}").into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Every note in `src` as (number, title, ids, unescaped body).
    fn notes(src: &str) -> Res<Vec<(u32, String, String, String)>> {
        let (_, mut r) = Reader::open(src.as_bytes())?;
        let mut out = vec![];
        while let Some(n) = r.next_note()? {
            out.push((n.n, n.title.to_string(), n.ids().collect::<Vec<_>>().join(","), n.body()));
        }
        Ok(out)
    }

    #[test]
    fn roundtrip_with_escapes() {
        let body = "line\n@@ 9 fake [x] header\n\\back\n\n# md";
        let mut src = format!("{MAGIC}\nid a: thing A\n\n").into_bytes();
        write_note(&mut src, 1, "2026-11-09T00:00:00Z", &["a"], "T", body).unwrap();
        let src = String::from_utf8(src).unwrap();
        let (head, _) = Reader::open(src.as_bytes()).unwrap();
        assert_eq!(head.ids().collect::<Vec<_>>(), [("a", "thing A")]);
        assert_eq!(notes(&src).unwrap(), [(1, "T".into(), "a".into(), body.into())]);
    }

    #[test]
    fn header_line_normalizes_ids_and_title() {
        let n = Note::parse("@@ 3 2026-11-09T10:00:00Z [a,,b]   spaced title  \nbody\n\n").unwrap();
        assert_eq!(n.to_string(), "3 2026-11-09T10:00:00Z [a,b] spaced title");
        let n = Note::parse("@@ 4 2026-11-09T10:00:00Z []\n").unwrap();
        assert_eq!(n.to_string(), "4 2026-11-09T10:00:00Z []");
    }

    #[test]
    fn crlf_missing_newline_and_extra_blank_lines() {
        let src = "notebook v1\r\nid a: A\r\n\r\n@@ 1 t [a] one\r\nx\r\n\r\n@@ 2 t [] two\nkept blank\n\n\n@@ 3 t [] three\nno newline";
        let got = notes(src).unwrap();
        assert_eq!(got[0], (1, "one".into(), "a".into(), "x".into()));
        assert_eq!(got[1].3, "kept blank\n"); // only one separator line is dropped
        assert_eq!(got[2].3, "no newline");
    }

    #[test]
    fn errors_name_the_line() {
        let err = |src: &str| notes(src).unwrap_err().to_string();
        assert_eq!(err("notebook v1\n\n@@ 1 t [] ok\n\n@@ x t [] bad\n"), "bad note header on line 5");
        assert_eq!(err("notebook v1\nbogus\n"), "unexpected line 2 in id region");
        assert_eq!(err(""), "not a notebook (expected 'notebook v1' on line 1)");
        assert_eq!(err("@@ 1 t [] x\n"), "not a notebook (expected 'notebook v1' on line 1)");
        assert!(notes("notebook v1\nuid ../escape\n").is_err());
        assert!(notes("notebook v1\nuid ..\n").is_err());
        let bad_utf8 = b"notebook v1\n\n@@ 1 t [] x\nok\n\xff\n";
        let (_, mut r) = Reader::open(&bad_utf8[..]).unwrap();
        assert_eq!(r.next_note().err().unwrap().to_string(), "invalid UTF-8 on line 5");
    }

    #[test]
    fn tiny_reads_split_notes_the_same() {
        let src = "notebook v1\n\n@@ 1 t [] one\n\\@@ not a header\n@@\n\n@@ 2 t [] two\nx\r\n\r\n@@ 3 t [] three\nend";
        let whole = notes(src).unwrap();
        for cap in 1..9 {
            let (_, mut r) = Reader::open(io::BufReader::with_capacity(cap, src.as_bytes())).unwrap();
            let mut got = vec![];
            while let Some(n) = r.next_note().unwrap() {
                got.push((n.n, n.title.to_string(), n.ids().collect::<Vec<_>>().join(","), n.body()));
            }
            assert_eq!(got, whole, "buffer of {cap} bytes");
        }
        assert_eq!(whole[0].3, "@@ not a header\n@@");
    }

    #[test]
    fn newline_search() {
        let text = b"0123456789abcdef\nxyz";
        for i in 0..text.len() {
            assert_eq!(newline(&text[i..]), text[i..].iter().position(|&b| b == b'\n'), "from {i}");
        }
    }

    #[test]
    fn rewind_starts_over() {
        let src = "notebook v1\nid a: A\n\n@@ 1 t [] one\n\n@@ 2 t [] two\n\n";
        let (_, mut r) = Reader::open(Cursor::new(src)).unwrap();
        while r.next_note().unwrap().is_some() {}
        r.rewind().unwrap();
        assert_eq!(r.next_note().unwrap().map(|n| n.n), Some(1));
        assert_eq!(r.next_note().unwrap().map(|n| n.raw.to_string()), Some("@@ 2 t [] two\n\n".into()));
    }
}
