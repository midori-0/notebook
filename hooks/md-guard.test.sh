#!/bin/sh
# Self-check for md-guard.sh: PASS or FAIL per case, exit 1 if anything failed.
g="$(dirname "$0")/md-guard.sh"; fail=0
check() { # $1 path, $2 expect: ask|allow
  out=$(printf '{"tool_input":{"file_path":"%s"}}' "$1" | "$g")
  got=allow; echo "$out" | grep -q '"ask"' && got=ask
  [ "$got" = "$2" ] && echo "PASS $2 $1" || { echo "FAIL want $2 got $got: $1"; fail=1; }
}
p=$(mktemp -d -t guardXXXX); mkdir -p "$p/docs" "$p/doc" # a project with real docs folders
check "$p/NOTES.md" ask
check "$p/plan.markdown" ask
check "$p/src/main.rs" allow
check "$p/README.md" allow
check "$p/AGENTS.md" allow
check "$p/docs/guide.md" allow
check "$p/doc/api/usage.md" allow
check "$p/docs/PLANNING.md" ask # notes by name, even in a real docs folder
check "$p/docs/build-plan.md" ask
check "$p/docs/handoff.md" ask
check "$p/docs/release-notes.md" allow
check "$p/new/docs/guide.md" ask # a docs folder that doesn't exist yet
check "$HOME/.claude/projects/x/memory/fact.md" allow
check "$HOME/.claude/plans/some-plan.md" allow
check "$p/.claude/skills/x/SKILL.md" allow
check "$0" allow # not markdown
f="$p/existing.md"; : > "$f"; check "$f" allow # markdown that already exists
rm -rf "$p"
exit $fail
