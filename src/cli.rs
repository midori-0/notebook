//! The command line: parses arguments, reads note bodies from stdin and prints results.

use crate::env::Env;
use crate::filter::Filter;
use crate::link::resolve;
use crate::read::{self, View};
use crate::{Res, remove, trash, untag, write};
use clap::{Parser, Subcommand};
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "notebook", version, about = "A searchable, single-file notebook for AI agents")]
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
    /// Remove identifiers from notes, or delete them from the notebook altogether
    #[command(visible_alias = "untag")]
    Rmid {
        /// Notebook (.nb) or link (.nt)
        file: PathBuf,
        /// Identifiers to remove
        #[arg(required = true)]
        ids: Vec<String>,
        /// Only from these notes, keeping the identifier defined (repeatable). Without this, the
        /// identifier is deleted from every note and from the dictionary
        #[arg(long = "note", short = 'n')]
        notes: Vec<u32>,
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

pub fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Res<()> {
    let env = Env::from_process();
    let mut out = BufWriter::new(io::stdout().lock());
    let s = |n: usize| if n == 1 { "" } else { "s" };
    match cli.cmd {
        Cmd::Make { file } => write::make(&file)?,
        Cmd::Add { file, ids, title, text } => {
            let nb = resolve(&file)?.0;
            let body = read_body(text.as_deref())?;
            writeln!(out, "{}", write::add(&env, &nb, &ids, title.as_deref(), &body)?)?;
        }
        Cmd::Edit { file, n, ids, title, text } => {
            let nb = resolve(&file)?.0;
            let body = text.as_deref().map(|t| read_body(Some(t))).transpose()?;
            write::edit(&env, &nb, n, &ids, title.as_deref(), body.as_deref())?;
        }
        Cmd::Rmid { file, ids, notes } => {
            let (removed, links) = untag::rmid(&env, &resolve(&file)?.0, &ids, &notes)?;
            for r in removed {
                match r.deleted {
                    Some(meaning) => writeln!(out, "deleted id {}: {meaning} (was on {} note{})", r.id, r.notes, s(r.notes))?,
                    None => writeln!(out, "removed id {} from {} note{}", r.id, r.notes, s(r.notes))?,
                }
            }
            for l in links {
                writeln!(out, "deleted link {l}")?;
            }
        }
        Cmd::Rm { file, notes } => {
            for l in remove::rm(&env, &resolve(&file)?.0, &notes)? {
                writeln!(out, "deleted link {l}")?;
            }
            trash::sweep(&env, false);
        }
        Cmd::Restore { file, notes } => {
            for (l, existed) in remove::restore(&env, &resolve(&file)?.0, &notes)? {
                if existed {
                    eprintln!("warning: {l} already exists; kept it");
                }
                writeln!(out, "restored link {l}")?;
            }
            trash::sweep(&env, false);
        }
        Cmd::Trash { file } => read::trash(&env, &file, &mut out)?,
        Cmd::Purge { orphans: true, .. } => {
            let (dirs, notes) = trash::sweep(&env, true);
            writeln!(out, "deleted {dirs} orphaned trash folder{}; {notes} expired note{}", s(dirs), s(notes))?;
        }
        Cmd::Purge { file, notes, .. } => {
            // clap guarantees a file here, and note numbers unless --all was passed.
            let n = remove::purge(&env, &resolve(&file.unwrap_or_default())?.0, &notes)?;
            writeln!(out, "purged {n} note{}", s(n))?;
            trash::sweep(&env, false);
        }
        Cmd::Ids { file } => read::ids(&file, &mut out)?,
        Cmd::List { file, filter } => read::print(&file, &filter, View::Headers, &mut out)?,
        Cmd::Show { file, filter } => read::print(&file, &filter, View::Full, &mut out)?,
        Cmd::Link { notebook, out: to, filter } => write::link(&env, &notebook, &to, &filter)?,
    }
    out.flush()?;
    Ok(())
}

/// The note body: `--text`, or stdin when that's absent or `-`. Trailing newlines are dropped.
/// Read before any lock is taken, so a slow pipe never holds up other writers.
fn read_body(text: Option<&str>) -> Res<String> {
    let mut b = match text {
        Some(t) if t != "-" => t.to_string(),
        _ if io::stdin().is_terminal() => return Err("no body: pass --text or pipe stdin".into()),
        _ => io::read_to_string(io::stdin())?,
    };
    b.truncate(b.trim_end_matches(['\n', '\r']).len());
    Ok(b)
}
