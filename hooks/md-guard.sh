#!/bin/sh
# Claude Code PreToolUse hook (matcher: Write). Asks the user before an agent creates a new
# .md file outside the allowlist, and points the agent at `nb`. Existing files, allowlisted
# names and allowlisted folders pass silently.

path=$(jq -r '.tool_input.file_path // empty')
case "$path" in *.md|*.MD|*.markdown) ;; *) exit 0 ;; esac
[ -e "$path" ] && exit 0

case "$(basename "$path" | tr '[:lower:]' '[:upper:]')" in
  README*|CHANGELOG*|CONTRIBUTING*|LICENSE*|SECURITY*|CODE_OF_CONDUCT*|CLAUDE.MD|CLAUDE.LOCAL.MD|AGENTS.MD|SKILL.MD) exit 0 ;;
esac
case "$path" in
  "$HOME"/.claude/*|*/.claude/*|*/docs/*|*/doc/*) exit 0 ;; # memory, plans, skills, docs
esac

jq -n --arg p "$path" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "ask",
  permissionDecisionReason: ("New markdown file " + $p + ". Agent notes belong in a notebook: nb add <subject>.nb -i <id> -t <title> (notebook skill). Approve only if you asked for this .md file.")}}'
