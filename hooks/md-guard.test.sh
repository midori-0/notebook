#!/bin/sh
# Self-check for md-guard.sh: PASS or FAIL per case, exit 1 if anything failed.
g="$(dirname "$0")/md-guard.sh"; fail=0
check() { # $1 path, $2 expect: ask|allow
  out=$(printf '{"tool_input":{"file_path":"%s"}}' "$1" | "$g")
  got=allow; echo "$out" | grep -q '"ask"' && got=ask
  [ "$got" = "$2" ] && echo "PASS $2 $1" || { echo "FAIL want $2 got $got: $1"; fail=1; }
}
check /proj/NOTES.md ask
check /proj/plan.markdown ask
check /proj/src/main.rs allow
check /proj/README.md allow
check /proj/docs/guide.md allow
check /proj/AGENTS.md allow
check "$HOME/.claude/projects/x/memory/fact.md" allow
check /proj/.claude/skills/x/SKILL.md allow
check "$0" allow # not markdown
f=$(mktemp -t guardXXXX).md; : > "$f"; check "$f" allow; rm -f "$f" # markdown that already exists
exit $fail
