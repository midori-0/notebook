//! End-to-end tests: commands run against real files in a scratch folder.

use crate::Res;
use crate::env::Env;
use crate::filter::Filter;
use crate::format::MAGIC;
use crate::paths::canon;
use crate::read::{self, View};
use crate::remove::{purge, restore, rm};
use crate::store;
use crate::trash::{self, Incoming, Trash};
use crate::untag::rmid;
use crate::write::{add, edit, link};
use std::fs;
use std::path::{Path, PathBuf};

/// A scratch folder with its own cache. Deleted when dropped, even if the test fails.
struct Scratch {
    root: PathBuf,
    env: Env,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("nb-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let env = Env { cache: root.join("cache"), now: crate::time::now(), trash_days: 30 };
        Scratch { root, env }
    }

    /// A notebook in the oldest format: just the magic line, no uid.
    fn notebook(&self, name: &str) -> PathBuf {
        let p = self.root.join(name);
        fs::write(&p, format!("{MAGIC}\n")).unwrap();
        p
    }

    fn trash(&self, nb: &Path) -> Trash {
        Trash::of(&self.env.cache, &store::uid(&store::header(nb).unwrap(), nb).unwrap())
    }

    fn add(&self, nb: &Path, ids: &[&str], title: &str, body: &str) -> Res<u32> {
        add(&self.env, nb, &strings(ids), Some(title), body)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn select(notes: Vec<u32>, ids: &[&str]) -> Filter {
    Filter { notes, ids: strings(ids), ..Default::default() }
}

/// Every note as (number, title, ids, body).
fn notes(nb: &Path) -> Vec<(u32, String, String, String)> {
    let (_, mut r) = store::open(nb).unwrap();
    let mut out = vec![];
    while let Some(n) = r.next_note().unwrap() {
        out.push((n.n, n.title.to_string(), n.ids().collect::<Vec<_>>().join(","), n.body()));
    }
    out
}

fn numbers(nb: &Path) -> Vec<u32> {
    notes(nb).into_iter().map(|n| n.0).collect()
}

#[test]
fn edit_in_place() {
    let sc = Scratch::new("edit");
    let p = sc.notebook("t.nb");
    sc.add(&p, &["a:thing A"], "one", "first").unwrap();
    sc.add(&p, &["a"], "two", "@@ escaped\nsecond").unwrap();
    sc.add(&p, &[], "three", "third").unwrap();

    edit(&sc.env, &p, 2, &[], Some("two v2"), None).unwrap(); // title only: the body keeps its escapes
    edit(&sc.env, &p, 3, &strings(&["b:thing B"]), None, Some("third v2")).unwrap();
    assert_eq!(edit(&sc.env, &p, 9, &[], Some("x"), None).unwrap_err().to_string(), "no note 9");
    assert_eq!(edit(&sc.env, &p, 9, &strings(&["new"]), None, None).unwrap_err().to_string(), "no note 9");
    assert!(edit(&sc.env, &p, 1, &[], None, None).is_err());
    assert!(edit(&sc.env, &p, 1, &[], Some("two\nlines"), None).is_err());

    let head = store::header(&p).unwrap();
    assert_eq!(head.ids().collect::<Vec<_>>(), [("a", "thing A"), ("b", "thing B")]);
    assert_eq!(
        notes(&p),
        [
            (1, "one".into(), "a".into(), "first".into()),
            (2, "two v2".into(), "a".into(), "@@ escaped\nsecond".into()),
            (3, "three".into(), "b".into(), "third v2".into()),
        ]
    );
    assert!(!sc.root.join("t.nb.tmp").exists()); // failed edits leave no temp file behind
}

#[test]
fn rm_and_restore() {
    let sc = Scratch::new("rm");
    let sub = sc.root.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let p = sc.notebook("t.nb");
    sc.add(&p, &["a:A", "b:B"], "one", "1").unwrap();
    sc.add(&p, &["a", "b"], "two", "2").unwrap();
    sc.add(&p, &["c:C"], "three", "3").unwrap();
    link(&sc.env, &p, &sub.join("n2.nt"), &select(vec![2], &[])).unwrap(); // names note 2: deleted
    link(&sc.env, &p, &sub.join("a.nt"), &select(vec![], &["a"])).unwrap(); // still selects note 1: kept
    link(&sc.env, &p, &sc.root.join("c.nt"), &select(vec![], &["c"])).unwrap(); // only note 3: deleted with it

    let mut gone = rm(&sc.env, &p, &[2, 3]).unwrap();
    gone.sort();
    assert_eq!(gone, ["c.nt", "sub/n2.nt"]);
    assert!(!sub.join("n2.nt").exists() && !sc.root.join("c.nt").exists() && sub.join("a.nt").exists());
    assert_eq!(rm(&sc.env, &p, &[2]).unwrap_err().to_string(), "no note 2");
    assert_eq!(sc.add(&p, &[], "four", "4").unwrap(), 4); // 2 and 3 are never reused

    assert_eq!(numbers(&p), [1, 4]);
    assert_eq!(sc.trash(&p).read(&[]).unwrap().numbers, [2, 3]);

    assert_eq!(restore(&sc.env, &p, &[2]).unwrap(), [("sub/n2.nt".to_string(), false)]);
    assert_eq!(numbers(&p), [1, 2, 4]);
    assert_eq!(fs::read_to_string(sub.join("n2.nt")).unwrap().lines().nth(1), Some("notebook: ../t.nb"));
    assert!(restore(&sc.env, &p, &[2]).is_err());
    assert_eq!(sc.trash(&p).read(&[]).unwrap().numbers, [3]);
    let mut links = store::header(&p).unwrap().links;
    links.sort();
    assert_eq!(links, ["sub/a.nt", "sub/n2.nt"]);
}

#[test]
fn restore_merges_in_number_order() {
    let sc = Scratch::new("merge");
    let p = sc.notebook("t.nb");
    for t in ["1", "2", "3", "4", "5"] {
        sc.add(&p, &[], t, t).unwrap();
    }
    rm(&sc.env, &p, &[1, 3, 5]).unwrap();
    restore(&sc.env, &p, &[5, 1]).unwrap();
    assert_eq!(numbers(&p), [1, 2, 4, 5]);
    restore(&sc.env, &p, &[3]).unwrap();
    assert_eq!(numbers(&p), [1, 2, 3, 4, 5]);
    assert!(!sc.trash(&p).dir().exists());
}

#[test]
fn purge_and_expiry() {
    let sc = Scratch::new("purge");
    let p = sc.notebook("t.nb");
    for t in ["1", "2", "3", "4"] {
        sc.add(&p, &[], t, t).unwrap();
    }
    link(&sc.env, &p, &sc.root.join("one.nt"), &select(vec![1], &[])).unwrap();
    rm(&sc.env, &p, &[1, 2, 3]).unwrap();

    let trash = sc.trash(&p);
    let in_trash = || trash.read(&[]).unwrap().numbers;
    let saved_links = || fs::read_dir(trash.dir()).map_or(0, |d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "nt")).count());
    assert_eq!((in_trash(), saved_links()), (vec![1, 2, 3], 1));

    assert_eq!(purge(&sc.env, &p, &[2]).unwrap(), 1);
    assert!(purge(&sc.env, &p, &[2]).is_err());
    assert_eq!(in_trash(), [1, 3]);

    // Nothing was removed before 2000; everything was removed before 9999.
    let expire = |cutoff| trash.update(trash.read(&[]).unwrap(), None, "", cutoff, |_| true, Incoming::default()).unwrap();
    assert_eq!(expire("2000"), 0);
    assert_eq!(in_trash(), [1, 3]);
    assert_eq!(expire("9999"), 2);
    assert!(!trash.dir().exists()); // the saved link copy went with its note

    rm(&sc.env, &p, &[4]).unwrap();
    assert_eq!(purge(&sc.env, &p, &[]).unwrap(), 1);
    assert!(!trash.dir().exists());
}

#[test]
fn sweep_rewrites_only_when_something_expires() {
    let mut sc = Scratch::new("sweep");
    let p = sc.notebook("t.nb");
    sc.add(&p, &[], "one", "1").unwrap();
    rm(&sc.env, &p, &[1]).unwrap();

    // A rewrite would drop this stray blank line, so its survival shows the file wasn't touched.
    let file = sc.trash(&p).dir().join("trash.nb");
    let marked = fs::read_to_string(&file).unwrap().replacen('\n', "\n\n", 1);
    fs::write(&file, &marked).unwrap();
    assert_eq!(trash::sweep(&sc.env, false), (0, 0));
    assert_eq!(fs::read_to_string(&file).unwrap(), marked);

    sc.env.trash_days = -1; // everything is past its expiry now
    assert_eq!(trash::sweep(&sc.env, false), (0, 1));
    assert!(!file.exists());
}

#[test]
fn trash_follows_moves_and_orphans_are_swept() {
    let sc = Scratch::new("move");
    fs::create_dir_all(sc.root.join("moved")).unwrap();
    let p = sc.notebook("t.nb");
    for t in ["1", "2"] {
        sc.add(&p, &[], t, t).unwrap();
    }
    let legacy = sc.trash(&p);
    rm(&sc.env, &p, &[1]).unwrap();
    assert!(fs::read_to_string(&p).unwrap().lines().nth(1).unwrap().starts_with("uid t.nb-")); // uid now stored

    let q = sc.root.join("moved/renamed.nb");
    fs::rename(&p, &q).unwrap();
    assert_eq!(sc.trash(&q).dir(), legacy.dir()); // same trash after a move and rename
    restore(&sc.env, &q, &[1]).unwrap();
    assert_eq!(numbers(&q), [1, 2]);

    // The owner's path is recorded, so deleting the notebook makes its trash an orphan.
    rm(&sc.env, &q, &[2]).unwrap();
    let owner = store::header(&legacy.dir().join("trash.nb")).unwrap().owner;
    assert_eq!(owner.as_deref(), canon(&q).unwrap().to_str());
    assert_eq!(trash::sweep(&sc.env, false), (0, 0)); // owner alive: nothing touched
    fs::remove_file(&q).unwrap();
    assert_eq!(trash::sweep(&sc.env, true), (1, 0));
    assert!(!legacy.dir().exists());
}

#[test]
fn list_and_show_through_a_link() {
    let sc = Scratch::new("show");
    let p = sc.notebook("t.nb");
    sc.add(&p, &["a:A"], "one", "\\starts with a backslash").unwrap();
    sc.add(&p, &["b:B"], "", "untitled").unwrap();
    sc.add(&p, &["a", "b"], "three", "x").unwrap();
    let nt = sc.root.join("a.nt");
    link(&sc.env, &p, &nt, &select(vec![], &["a"])).unwrap();

    let run = |path: &Path, f: Filter, view| {
        let mut out = vec![];
        read::print(path, &f, view, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let ts = sc.env.stamp();
    assert_eq!(run(&p, select(vec![], &["b"]), View::Headers), format!("2 {ts} [b]\n3 {ts} [a,b] three\n"));
    assert_eq!(run(&nt, select(vec![], &["b"]), View::Headers), format!("3 {ts} [a,b] three\n"));
    assert_eq!(run(&nt, select(vec![1], &[]), View::Full), format!("@@ 1 {ts} [a] one\n\\starts with a backslash\n\n"));
}

#[test]
fn a_note_bigger_than_the_buffers_survives() {
    let sc = Scratch::new("big");
    let p = sc.notebook("t.nb");
    let big: String = (0..20_000).map(|i| format!("line {i} @@ not a header\n@@ {i} escaped\n")).collect();
    let big = big.trim_end();
    sc.add(&p, &["a:A"], "big", big).unwrap();
    sc.add(&p, &[], "after", "small").unwrap();
    edit(&sc.env, &p, 1, &[], Some("big, renamed"), None).unwrap();
    let got = notes(&p);
    assert_eq!((got[0].1.as_str(), got[0].3.as_str()), ("big, renamed", big));
    assert_eq!(got[1].3, "small");
}

#[test]
fn rmid_untags_chosen_notes_or_deletes_everywhere() {
    let sc = Scratch::new("rmid");
    let p = sc.notebook("t.nb");
    sc.add(&p, &["a:A", "b:B"], "one", "1").unwrap();
    sc.add(&p, &["a", "c:C"], "two", "2").unwrap();
    sc.add(&p, &["b"], "three", "3").unwrap();
    link(&sc.env, &p, &sc.root.join("a.nt"), &select(vec![], &["a"])).unwrap(); // filters on a: goes when a is deleted
    link(&sc.env, &p, &sc.root.join("one.nt"), &select(vec![1], &[])).unwrap(); // by number: stays
    link(&sc.env, &p, &sc.root.join("c.nt"), &select(vec![], &["c"])).unwrap(); // only note 2 has c: emptied
    link(&sc.env, &p, &sc.root.join("b.nt"), &select(vec![], &["b"])).unwrap(); // note 1 keeps b: stays
    let later = Env { cache: sc.env.cache.clone(), now: sc.env.now + 3600, trash_days: 30 };
    let ids = |p: &Path| notes(p).into_iter().map(|n| n.2).collect::<Vec<_>>();
    let dictionary = |p: &Path| store::header(p).unwrap().ids.into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    let summary = |r: Vec<crate::untag::Removed>| r.into_iter().map(|r| (r.id, r.deleted, r.notes)).collect::<Vec<_>>();

    // From chosen notes only: the identifiers stay defined.
    let (removed, links) = rmid(&later, &p, &strings(&["c", "b", "c"]), &[2, 3]).unwrap();
    assert_eq!(summary(removed), [("c".into(), None, 1), ("b".into(), None, 1)]);
    assert_eq!(links, ["c.nt"]);
    assert!(!sc.root.join("c.nt").exists() && sc.root.join("b.nt").exists());
    assert_eq!(ids(&p), ["a,b", "a", ""]);
    assert_eq!(dictionary(&p), ["a", "b", "c"]);

    // Everywhere: out of the dictionary and every note, and links filtering on it go too.
    let (removed, links) = rmid(&later, &p, &strings(&["a"]), &[]).unwrap();
    assert_eq!(summary(removed), [("a".into(), Some("A".into()), 2)]);
    assert_eq!(links, ["a.nt"]);
    assert_eq!(ids(&p), ["b", "", ""]);
    assert_eq!(dictionary(&p), ["b", "c"]);
    assert!(sc.root.join("one.nt").exists());
    let (_, mut r) = store::open(&p).unwrap();
    while let Some(n) = r.next_note().unwrap() {
        assert_eq!(n.ts, sc.env.stamp()); // timestamps don't move
    }

    assert_eq!(rmid(&sc.env, &p, &strings(&["zzz"]), &[]).unwrap_err().to_string(), "no id 'zzz'");
    assert_eq!(rmid(&sc.env, &p, &strings(&["b"]), &[9]).unwrap_err().to_string(), "no note 9");
    let untouched = fs::read_to_string(&p).unwrap();
    let (removed, _) = rmid(&sc.env, &p, &strings(&["b"]), &[2]).unwrap(); // note 2 doesn't carry b
    assert_eq!(removed[0].notes, 0);
    assert_eq!(fs::read_to_string(&p).unwrap(), untouched);
}

#[test]
fn restore_drops_identifiers_deleted_meanwhile() {
    let sc = Scratch::new("rmid-restore");
    let p = sc.notebook("t.nb");
    sc.add(&p, &["a:A", "b:B"], "one", "1").unwrap();
    sc.add(&p, &["a"], "two", "2").unwrap();
    rm(&sc.env, &p, &[1]).unwrap();
    rmid(&sc.env, &p, &strings(&["a"]), &[]).unwrap();
    restore(&sc.env, &p, &[1]).unwrap();
    assert_eq!(notes(&p), [(1, "one".into(), "b".into(), "1".into()), (2, "two".into(), "".into(), "2".into())]);
}
