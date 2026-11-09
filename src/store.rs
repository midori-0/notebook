//! Opening, locking and atomically replacing notebook files.

use crate::Res;
use crate::format::{Header, Reader};
use crate::paths::{at, canon, file_name, fnv};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Read and write buffer size. Big enough to keep syscalls rare, small enough not to matter.
const BUF: usize = 64 * 1024;

pub type Notes = Reader<BufReader<File>>;

/// A notebook's header, and a reader positioned at its first note.
pub fn open(path: &Path) -> Res<(Header, Notes)> {
    read(File::open(path).map_err(|e| at(path, e))?)
}

/// Like `open`, for a file that's already open.
pub fn read(file: File) -> Res<(Header, Notes)> {
    Reader::open(BufReader::with_capacity(BUF, file))
}

/// Just the header. Cheap however many notes follow it.
pub fn header(path: &Path) -> Res<Header> {
    let file = File::open(path).map_err(|e| at(path, e))?;
    Ok(Reader::open(BufReader::new(file))?.0)
}

/// The notebook's uid. Notebooks made before uids existed get the path-based name their trash
/// already uses; the next rewrite stores it in the file, so it stays put after that.
pub fn uid(head: &Header, path: &Path) -> Res<String> {
    if let Some(u) = &head.uid {
        return Ok(u.clone());
    }
    let abs = canon(path)?;
    Ok(format!("{}-{:016x}", file_name(&abs), fnv(abs.as_os_str().as_encoded_bytes())))
}

/// A fresh uid, `<file name>-<16 hex digits>`, different for every notebook made.
pub fn new_uid(path: &Path) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let seed = format!("{nanos}-{}-{}", std::process::id(), path.display());
    format!("{}-{:016x}", file_name(path), fnv(seed.as_bytes()))
}

/// A replacement for a file, written beside it and swapped in by `commit`. Dropped without a
/// commit, it deletes itself and leaves the original untouched.
pub struct Rewrite {
    out: BufWriter<File>,
    tmp: PathBuf,
    path: PathBuf,
    done: bool,
}

impl Rewrite {
    pub fn create(path: &Path) -> Res<Rewrite> {
        let tmp = path.with_extension("nb.tmp");
        let out = BufWriter::with_capacity(BUF, File::create(&tmp)?);
        Ok(Rewrite { out, tmp, path: path.to_path_buf(), done: false })
    }

    /// Makes the new contents durable, then renames them over the original in one step, so
    /// readers see the old file or the new one and never half of each. Close any reader of the
    /// original first: Windows won't replace a file this process still has open.
    pub fn commit(mut self) -> Res<()> {
        self.out.flush()?;
        self.out.get_ref().sync_all()?;
        fs::rename(&self.tmp, &self.path)?;
        self.done = true;
        Ok(())
    }
}

impl Write for Rewrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.write(buf)
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.out.write_all(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl Drop for Rewrite {
    fn drop(&mut self) {
        if !self.done {
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

/// A notebook's write lock, held until dropped.
///
/// The lock is taken on a separate file in the cache, never on the notebook. That leaves the
/// notebook free for readers (Windows locks are mandatory) and for atomic replacement.
pub struct Lock {
    _file: File,
}

impl Lock {
    /// Waits for the lock.
    pub fn wait(cache: &Path, notebook: &Path) -> Res<Lock> {
        let f = lock_file(cache, notebook)?;
        f.lock()?;
        Ok(Lock { _file: f })
    }

    /// Takes the lock only if it's free.
    pub fn try_now(cache: &Path, notebook: &Path) -> Option<Lock> {
        let f = lock_file(cache, notebook).ok()?;
        f.try_lock().ok()?;
        Some(Lock { _file: f })
    }
}

fn lock_file(cache: &Path, notebook: &Path) -> Res<File> {
    let abs = canon(notebook).map_err(|e| at(notebook, e))?;
    let dir = cache.join(".locks");
    fs::create_dir_all(&dir)?;
    let name = format!("{:016x}.lock", fnv(abs.as_os_str().as_encoded_bytes()));
    Ok(OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(name))?)
}
