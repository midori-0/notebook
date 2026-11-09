//! Path helpers that give the same answers on macOS, Linux and Windows.

use std::fmt::Display;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Canonical absolute path. On Windows the `\\?\` prefix is dropped from plain drive paths, so
/// the result still joins with `/`-separated relative paths.
pub fn canon(p: &Path) -> io::Result<PathBuf> {
    let c = fs::canonicalize(p)?;
    #[cfg(windows)]
    if let Some(s) = c.to_str().and_then(|s| s.strip_prefix(r"\\?\")).filter(|s| s.as_bytes().get(1) == Some(&b':')) {
        return Ok(PathBuf::from(s));
    }
    Ok(c)
}

/// The folder a notebook's links are stored relative to.
pub fn folder(abs: &Path) -> Result<&Path, &'static str> {
    abs.parent().ok_or("notebook has no parent folder")
}

/// The path from `from_dir` to `to`, both absolute.
pub fn relative(from_dir: &Path, to: &Path) -> PathBuf {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut p: PathBuf = std::iter::repeat_n("..", a.len() - common).collect();
    p.extend(&b[common..]);
    p
}

/// `p` with `/` between components on every OS, so stored paths work wherever they're read.
pub fn slash(p: &Path) -> String {
    let mut s = String::new();
    for (i, c) in p.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(&c.to_string_lossy());
    }
    s
}

pub fn file_name(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// An error message that names the file it's about.
pub fn at(p: &Path, e: impl Display) -> String {
    format!("{}: {e}", p.display())
}

/// FNV-1a. Unlike `DefaultHasher` it never changes between Rust versions, and lock and trash
/// names depend on that.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/a/b/c"), Path::new("/a/x/n.nb")), PathBuf::from("../../x/n.nb"));
        assert_eq!(relative(Path::new("/a"), Path::new("/a/n.nb")), PathBuf::from("n.nb"));
        assert_eq!(slash(Path::new("a/b/c.nt")), "a/b/c.nt");
    }
}
