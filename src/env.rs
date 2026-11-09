//! What commands need from outside the notebook: the cache folder, the time and the trash
//! retention. Commands take an `Env` rather than reading globals, so tests can aim them at a
//! scratch folder.

use crate::time;
use std::path::PathBuf;

pub struct Env {
    /// Lock files and every notebook's trash live here.
    pub cache: PathBuf,
    /// Read once, so everything one command writes carries the same time.
    pub now: i64,
    /// Trash entries older than this expire.
    pub trash_days: i64,
}

impl Env {
    /// The cache is `$XDG_CACHE_HOME`, else `~/.cache` (`%LOCALAPPDATA%` on Windows), plus
    /// `notebook`. Retention comes from `NB_TRASH_DAYS`, default 30.
    pub fn from_process() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let os_default = if cfg!(windows) { var("LOCALAPPDATA") } else { var("HOME").map(|h| h.join(".cache")) };
        let root = var("XDG_CACHE_HOME").or(os_default).unwrap_or_else(std::env::temp_dir);
        let trash_days = std::env::var("NB_TRASH_DAYS").ok().and_then(|d| d.parse().ok()).unwrap_or(30);
        Env { cache: root.join("notebook"), now: time::now(), trash_days }
    }

    pub fn stamp(&self) -> String {
        time::format(self.now)
    }

    /// Trash entries removed before this time have expired.
    pub fn cutoff(&self) -> String {
        time::format(self.now.saturating_sub(self.trash_days.saturating_mul(86400)))
    }
}
