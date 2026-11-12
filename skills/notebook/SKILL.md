---
name: notebook
description: Use the `nb` notebook CLI instead of loose markdown files for an agent's own notes, plans, findings, progress logs, decisions, TODOs, research and handoffs. Use it before creating any scratch or notes .md file, when recording what you learned or decided, when resuming work, and before re-reading large files you may have summarized already. Do not use it for files that people or tools expect as markdown (README, CHANGELOG, docs/, CLAUDE.md, AGENTS.md). The user switches notebook mode with /notebook on, /notebook off and /notebook status.
argument-hint: "[on|off|status]"
allowed-tools: Bash(${CLAUDE_SKILL_DIR}/mode.sh *)
---

# notebook (`nb`)

`nb` stores many small markdown notes in one `.nb` file. Each note has a number, an automatic timestamp, identifiers and a title. Every identifier is defined once, with its meaning, in a dictionary at the top of the file. Read the dictionary and the headers first, then read only the notes you need, instead of re-reading whole files.

If `nb` is not on PATH, tell the user and stop. It installs with `cargo install --path <notebook repo>`.

## Notebook mode: `/notebook on`, `off`, `status`

You were invoked with: `$ARGUMENTS`

If the user typed `/notebook on`, `/notebook off` or `/notebook status`, run `${CLAUDE_SKILL_DIR}/mode.sh on` (or `off`, or `status`) and tell the user the mode in one line. The mode is global: it covers every project and every new session.

- **on**: follow this guide for the rest of the session. New sessions start with the notebook rule, and creating a new `.md` file outside the allowlist needs the user's approval.
- **off**: from now on, ignore this guide and any notebook rule given earlier in the session. Write markdown as you normally would, and use `nb` only when the user asks for it. New sessions start without the rule, and new `.md` files go through as usual.
- **status**: just report it.

Anything else after `/notebook`: say the choices are `on`, `off` and `status`, and change nothing. Without `/notebook`, you were loaded to keep notes, so carry on with the guide below. If notebook mode was switched off during this session, use `nb` only when the user asks for it.

## What a notebook is for

A notebook is an organizing tool, not a dump. It exists so that you, or the next agent, can find the right knowledge without re-reading things already read, and so that related information sits together.

- **Don't record everything.** Write a note when it will save a later read or keep a decision from being lost: a summary of a large file, a finding, a decision and its reason, a handoff. Skip trivia, command output, and anything that is quick to re-derive or is already clear from the code.
- **Organize so reads stay narrow.** Choose identifiers so that someone looking for one topic can pull just that topic. An agent looking for single-celled organisms should be able to run `nb list biology.nb -i single-celled` and never see the octopus notes.

## When to use it

Use `nb` for anything you would otherwise write as a markdown file for yourself or other agents: plans, findings, investigation notes, decisions and their reasons, progress or handoff notes, TODOs, summaries of files or docs you've read.

Keep regular markdown for files that humans or tools read directly: README, CHANGELOG, docs sites, CLAUDE.md/AGENTS.md, and anything the user asked for as a `.md` file.

A plan, brief or handoff written for another agent to read is a note too, even when the user calls it a "planning doc" and even if a `docs/` folder is handy. Creating `docs/` just to hold it doesn't make it a docs site. If it needs to sit in a particular folder, use an `.nt` link (below). If you're unsure whether the user wants a `.md` file, ask.

## Which notebook

Use one notebook per **subject area**, not one per project. If a project covers unrelated fields, give each field its own notebook rather than forcing them into one. For example, a project researching both computer science and biology gets `computer-science.nb` and `biology.nb`. Within a notebook, identifiers split the subject into topics (`single-celled`, `cephalopods`, ...).

1. Look for existing notebooks first: `find . -name '*.nb' -not -path '*/target/*' -not -path '*/node_modules/*'`. Check `nb ids` on the likely one to confirm it covers your subject.
2. Add to an existing notebook when the subject matches. Create a new one (`nb make <subject>.nb`) only for a genuinely separate subject. Put it at the project root, or in the folder that subject's work lives in.
3. Name notebooks after their subject. Don't create one per task, per session or per folder: a task's notes belong in the subject notebook, under the right identifiers.
4. If a single notebook's `nb ids` lists topics that never relate to each other, it should probably be two notebooks. Mention this to the user instead of splitting it yourself.

## Start of every task: read before you read files

```bash
nb ids <subject>.nb                      # the vocabulary: what each identifier means
nb list <subject>.nb -i <id>              # headers only: number, time, ids, title
nb list <subject>.nb --since 2026-11-09   # what changed recently (any timestamp prefix)
nb show <subject>.nb -n 12 -n 15          # full text of just the notes you need
```

If a note already summarizes a file, use the note. Don't open the file again unless you need detail the note doesn't have or the file may have changed.

## Writing notes

```bash
nb add backend.nb -i auth -i 'perf:Performance measurements and hotspots' -t "Login p99 is 800ms" --text "Measured with wrk. Most of the time is in bcrypt cost 14."
```

Multi-line bodies go through stdin, which avoids quoting problems:

```bash
nb add backend.nb -i auth -t "Token refresh design" <<'EOF'
- Refresh tokens rotate on every use.
- Reuse of an old token revokes the family.
EOF
```

Rules:
- **Reuse identifiers.** Run `nb ids` and use an existing identifier when one fits (`-i auth`). Define a new one only for a genuinely new topic, as `-i 'name:one-line meaning'`. The tool rejects a new identifier with no meaning.
- **Identifier names:** short, lowercase, letters, digits, `_ - .` (`auth`, `db-migration`, `todo`, `decision`). Name topics or kinds of note, not single notes.
- **Always give a title** (`-t`). `nb list` shows only headers, so the title is what lets later readers skip the body.
- **One fact or decision per note.** Small notes make selective reads work. Don't append a whole session's log as one note.
- **Edit notes that are wrong or out of date instead of adding near-duplicates.** `nb edit <file> <n>` replaces only what you pass: `--text` (or `--text -` for stdin), `-t`, or `-i` (which replaces the whole identifier list). The note keeps its number and its timestamp becomes now, so `--since` finds edits too. Add a new note when there's a genuinely new fact or decision. Edit when it corrects or refines an existing one.

```bash
nb edit backend.nb 7 -t "Token TTL is 15m" --text "Changed from 1h after the audit."
```
- `nb add` prints the new note's number. Mention it when you report back (e.g. "recorded as note #23").

Useful general identifiers to reuse across projects (define them when first needed): `decision`, `todo`, `finding`, `handoff`, `summary`.

## Removing notes

Remove a note only when it is wrong and not worth correcting, a duplicate, or no longer relevant. If it is only out of date, edit it instead.

```bash
nb rm backend.nb 7 9      # prints any .nt links it deleted
nb trash backend.nb       # what can be restored
nb restore backend.nb 7   # undo: the note comes back with its number and its links
```

`rm` also deletes `.nt` links that name a removed note, or that would select nothing afterwards. Mention any deleted links in your report. If you removed something by mistake, run `nb restore` straight away. Removed notes stay in the trash for 30 days.

`nb purge` (specific numbers, `--all`, or `--orphans` for trash of deleted notebooks) deletes from the trash permanently. Run it only when the user asks for it.

## Removing identifiers

Delete an identifier that was a mistake, a near-duplicate of another, or a topic that no longer exists. Take one off a note that was tagged wrongly.

```bash
nb rmid backend.nb authn         # out of the dictionary and every note
nb rmid backend.nb perf -n 12    # off note 12 only; perf stays defined
```

Timestamps don't change. `rmid` (alias `untag`) also deletes `.nt` links that filter on a deleted identifier, or would select nothing afterwards, and prints them. There's no undo, so before deleting a near-duplicate, retag its notes with the identifier you're keeping (`nb edit <file> <n> -i ...`).

## Sharing a selection in a folder: `.nt` links

When information belongs next to specific code, or another agent or person needs it in a particular folder, don't copy it into a new markdown file. Create a link:

```bash
nb link backend.nb services/auth/auth.nt -i auth   # all auth notes
nb link backend.nb handoff.nt -n 30 -n 31          # specific notes
```

A `.nt` file works anywhere a `.nb` does: `nb show services/auth/auth.nt` prints that selection, `nb list` lists it, and `nb add services/auth/auth.nt ...` writes into the parent notebook. When you find a `.nt` file, read it with `nb show <file>.nt`.

## Filters

- `-n N` (repeatable): note numbers. A note matches if it is any of them.
- `-i ID` (repeatable): the note must carry **all** of the given identifiers.
- `--since T` / `--until T`: inclusive bounds. Any prefix of `2026-11-09T12:00:00Z` works.

## Don't

- Don't edit a `.nb` file by hand or with sed, and don't delete `.nt` files with `rm`. Use `nb add`, `nb edit` and `nb rm` (they lock the file and keep the dictionary, link registry and trash consistent).
- Don't `cat` a whole notebook. Use `nb ids`, then `nb list`, then `nb show -n`.
- Don't create `NOTES.md`, `PLAN.md`, `progress.md`, `findings.md` or similar files for your own use.
