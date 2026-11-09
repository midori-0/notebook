//! Selecting notes by number, identifier and time.

use crate::format::Note;
use clap::Args;

/// A selection of notes. Each part left empty matches everything.
#[derive(Args, Default, Debug, PartialEq)]
pub struct Filter {
    /// Note number (repeatable; any match)
    #[arg(long = "note", short = 'n')]
    pub notes: Vec<u32>,
    /// Identifier the note must carry (repeatable; all must match)
    #[arg(long = "id", short = 'i')]
    pub ids: Vec<String>,
    /// Earliest timestamp, inclusive; any prefix works (e.g. 2026-11)
    #[arg(long)]
    pub since: Option<String>,
    /// Latest timestamp, inclusive; any prefix works (e.g. 2026-11-09)
    #[arg(long)]
    pub until: Option<String>,
}

impl Filter {
    pub fn matches(&self, n: &Note) -> bool {
        self.matches_with(n, |i| n.has_id(i))
    }

    /// `matches`, with `has_id` deciding which identifiers the note carries.
    pub fn matches_with(&self, n: &Note, has_id: impl Fn(&str) -> bool) -> bool {
        (self.notes.is_empty() || self.notes.contains(&n.n))
            && self.ids.iter().all(|i| has_id(i))
            && self.since.as_deref().is_none_or(|s| n.ts >= s)
            // Compare only as much of the timestamp as `until` spells out, so `--until 2026-11`
            // includes all of November.
            && self.until.as_deref().is_none_or(|u| n.ts.get(..u.len()).unwrap_or(n.ts) <= u)
    }
}

/// Note numbers, sorted so a lookup stays cheap however many are given.
pub struct Numbers(Vec<u32>);

impl Numbers {
    pub fn new(ns: &[u32]) -> Self {
        let mut v = ns.to_vec();
        v.sort_unstable();
        v.dedup();
        Numbers(v)
    }

    pub fn has(&self, n: u32) -> bool {
        self.0.binary_search(&n).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_numbers_ids_and_time() {
        let n = Note::parse("@@ 3 2026-11-09T10:00:00Z [a,b] t").unwrap();
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
}
