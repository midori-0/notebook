#!/bin/sh
# Notebook mode: while it's on, agents keep their own notes in `nb` and new .md files need the
# user's approval. Usage: mode.sh [on|off|status]; prints the mode, after any change.
# Stored in ~/.claude/notebook-mode (NB_MODE_FILE overrides). No file means on.

file="${NB_MODE_FILE:-$HOME/.claude/notebook-mode}"
case "$1" in
  on|off) mkdir -p "$(dirname "$file")" && printf '%s\n' "$1" > "$file" || exit 1 ;;
  status|"") ;;
  *) echo "usage: mode.sh [on|off|status]" >&2; exit 2 ;;
esac
[ "$(cat "$file" 2>/dev/null)" = off ] && echo off || echo on
