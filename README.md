# notebook

**A searchable, single-file notebook for AI agents.**

`notebook` (alias `nb`) replaces the pile of `NOTES.md`, `PLAN.md` and `findings-v2.md` files that agents leave behind. Each subject gets one `.nb` file holding many small notes. Every note carries a number, a timestamp, identifiers and a title. Every identifier is defined once, with its meaning, at the top of the file.

An agent can learn what a notebook knows from its dictionary and headers, a few hundred tokens, and then read only the notes it needs. It doesn't have to re-read whole files it has already seen.

```text
$ nb ids biology.nb
cells: Single-celled organisms
octo: Octopus biology

$ nb list biology.nb -i cells
1 2026-11-09T12:56:12Z [cells] Amoeba movement
3 2026-11-09T14:02:40Z [cells] Paramecium movement

$ nb show biology.nb -n 3
@@ 3 2026-11-09T14:02:40Z [cells] Paramecium movement
Paramecia swim by beating thousands of cilia in coordinated waves.
```

Looking for single-celled organisms never means reading the octopus notes.

## Why

- **Fewer redundant reads.** Headers and the identifier dictionary are a cheap index, so agents read the index first and pull exact notes with `-n`, `-i`, `--since`.
- **Less clutter.** One notebook per subject instead of scattered markdown files. `.nt` link files put a selection of notes in a specific folder without copying anything.
- **Shared vocabulary.** A new identifier must come with a one-line meaning. Agents reuse identifiers instead of inventing near-duplicates.
- **Fast and safe.** Written in Rust, with no runtime dependencies beyond `clap`. Commands stream a notebook one note at a time: about 5 ms on a 10,000-note notebook, and about 2 MB of memory however big it gets. Writes lock the file, so many agents can write at once. Rewrites are atomic.

## Install

```bash
cargo install --git https://github.com/midori-0/notebook
```

Or, from a clone, `cargo install --path .`. Both install `notebook` and `nb`. Works on macOS, Linux and Windows.

## Quick start

```bash
nb make biology.nb
nb add biology.nb -i 'cells:Single-celled organisms' -t "Amoeba movement" --text "Amoebas move with pseudopods."
nb add biology.nb -i cells -t "Paramecium movement" <<'EOF'
Paramecia swim using thousands of cilia.
EOF
nb list biology.nb -i cells
nb edit biology.nb 1 --text "Amoebas move by extending pseudopods."
```

`nb add` prints the new note's number. With no `--text`, the body is read from stdin.

## Commands

**Write**

| Command | What it does |
|---|---|
| `nb make <file.nb>` | Create an empty notebook |
| `nb add <file> [-i ID]... [-t TITLE] [--text BODY]` | Add a note and print its number. New identifiers must be `-i 'name:meaning'`; existing ones are just `-i name` |
| `nb edit <file> <n> [-t ...] [--text ...] [-i ...]` | Edit a note in place. Only the parts you pass change. `--text -` reads stdin, and `-i` replaces the identifier list. The timestamp becomes now |
| `nb link <file.nb> <out.nt> [filters]` | Write a `.nt` link to a selection of notes |

**Read**

| Command | What it does |
|---|---|
| `nb ids <file>` | Print the identifier dictionary |
| `nb list <file> [filters]` | Print note headers only (the cheap index) |
| `nb show <file> [filters]` | Print full notes |

**Remove and recover**

| Command | What it does |
|---|---|
| `nb rm <file> <n>...` | Remove notes and delete the `.nt` links that point at them. Both go to the trash |
| `nb trash <file>` | List removed notes, when each was removed, and their deleted links |
| `nb restore <file> <n>...` | Put notes back with their original numbers, and recreate their links |
| `nb purge <file> <n>...` / `--all` | Permanently delete from the trash. Cannot be undone |
| `nb purge --orphans` | Delete trash left by notebooks that no longer exist |

**Filters** for `list`, `show` and `link`:

- `-n 3 -n 5`: note numbers; a note matches if it is any of them.
- `-i api -i bug`: the note must carry all of these identifiers.
- `--since 2026-11` and `--until 2026-11-09`: inclusive time bounds. Any prefix of a timestamp works.

Every command accepts a `.nt` link wherever it takes a notebook. `nb show docs/api.nt` prints the link's selection, and `nb add docs/api.nt ...` writes to the notebook it links to.

## Links (`.nt`)

When information belongs next to specific code, don't copy it into a new markdown file. Link to it:

```bash
nb link backend.nb services/auth/auth.nt -i auth    # every auth note
nb link backend.nb handoff.nt -n 30 -n 31           # specific notes
```

A link stores a relative path to its notebook plus a selection. It keeps working when the repository moves, and anyone who finds it reads it with `nb show <file>.nt`. Notebooks keep a record of their links, so `nb rm` can clean up after itself:
- it deletes links that name a removed note by number, and links whose selection would become empty;
- group links that still match other notes are kept.

## Trash

Removed notes and deleted links go to `~/.cache/notebook/<uid>/` on macOS and Linux, `%LOCALAPPDATA%\notebook\<uid>\` on Windows, or under `$XDG_CACHE_HOME` when that's set.
- **Restoring:** `nb restore` brings a note back in its original place with its original number. Note numbers are never reused, so links can't silently point at the wrong note.
- **Expiry:** entries expire after 30 days; set `NB_TRASH_DAYS` to change that. Each `rm`, `restore` or `purge` sweeps expired entries from every notebook's trash.
- **Moves:** each notebook has a permanent `uid`, so its trash follows it when it's moved or renamed.

## Using it with agents

### Claude Code skill

`skills/notebook/` teaches agents when and how to use `nb`:
- what belongs in a notebook, and what doesn't;
- one notebook per subject;
- reading the index before reading notes;
- reusing identifiers;
- editing a note rather than duplicating it.

Enable it everywhere:

```bash
ln -s "$PWD/skills/notebook" ~/.claude/skills/notebook
```

For a stronger nudge, add a line like this to `~/.claude/CLAUDE.md`:

> For your own notes, plans, findings, TODOs, research and handoffs, use the `nb` notebook CLI (see the notebook skill), never new `.md` files. That includes plans, briefs and handoffs written for another agent, even when I call them a doc. Plain markdown is only for README, CHANGELOG, an existing docs site, CLAUDE.md, AGENTS.md, skills, memory, or when I ask for a `.md` file.

### Guard hook

`hooks/md-guard.sh` is a `PreToolUse` hook (matcher `Write`). It asks for your approval before an agent creates a new `.md` file outside an allowlist:
- README, CHANGELOG, CONTRIBUTING, LICENSE, CLAUDE.md, AGENTS.md and SKILL.md;
- anything under `.claude/`, or under a `docs/` folder that already exists.

Notes by name (`PLAN.md`, `PLANNING.md`, `NOTES.md`, `TODO.md`, `handoff.md` and the like) are asked about even inside `docs/`, and so is a `docs/` folder created for the new file.

Editing existing markdown is never blocked. It's a shell script that needs `jq`; on Windows it runs under the Git Bash that Claude Code uses. Add it to `~/.claude/settings.json`:

```json
{ "hooks": { "PreToolUse": [ { "matcher": "Write",
  "hooks": [ { "type": "command", "command": "/path/to/notebook/hooks/md-guard.sh" } ] } ] } }
```

### Native edit cards (`nb-diff` mod)

`plugins/nb-diff/` is a Claude Code mod. It shows every notebook write (`make`, `add`, `edit`, `rm`, `restore`, `link`) as Claude Code's own **Edit** card for the `.nb` file, with line numbers and red/green changes, exactly like a code edit. It changes the display only: the agent sees the same output, and token use is unchanged.

This repository is its marketplace:

```bash
claude plugin install nb-diff --marketplace midori-0/notebook
```

Or, from a clone, run `claude plugin marketplace add /path/to/notebook` and then `claude plugin install nb-diff@notebook`.

## File format

A notebook is plain text:

```text
notebook v1
uid biology.nb-64c6f45652a04287
id cells: Single-celled organisms
last 4
link docs/cells.nt

@@ 1 2026-11-09T12:56:12Z [cells] Amoeba movement
Amoebas move by extending pseudopods.

```

- **`uid`:** a permanent id. It names the notebook's trash folder.
- **`id name: meaning`:** the identifier dictionary.
- **`last N`:** the highest note number ever used, so removed numbers are never reused.
- **`link <path>`:** the registered `.nt` files, relative to the notebook's folder.
- **`@@ <n> <timestamp> [ids] <title>`:** a note header. The markdown body follows it.

Body lines that start with `@@` or `\` are stored with an extra leading `\`. Paths stored in links always use `/`, so a notebook and its links can move between macOS, Linux and Windows. Writers take a lock on a small file in the cache folder, never on the notebook itself, so readers can always open a notebook while it's being written. Timestamps are UTC RFC 3339, so they sort as plain text, which is why `--since` and `--until` take any prefix.

## Development

```bash
cargo test                              # format, filters, edit, remove/restore, trash expiry, moves (CI: macOS, Linux, Windows)
cargo clippy --all-targets
hooks/md-guard.test.sh                  # guard hook self-check
claude plugin test plugins/nb-diff      # mod tests
```

```text
src/format.rs           the .nb format and its streaming reader
src/store.rs            opening, locking and atomically replacing files
src/trash.rs            the trash: removed notes, expiry, orphan sweeps
src/read.rs, write.rs, remove.rs   the commands
src/cli.rs              argument parsing and output
src/main.rs, bin/nb.rs  the `notebook` and `nb` binaries
skills/notebook/        Claude Code skill
hooks/                  .md guard hook and its self-check
plugins/nb-diff/        Claude Code mod: notebook writes as native Edit cards
```

## Limitations

- No full-text search over note bodies. Find notes by identifier, number and time.
