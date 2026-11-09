#!/bin/sh
# Claude Code PreToolUse hook (matcher: Write). Asks the user before an agent creates a new
# .md file outside the allowlist, and points the agent at `nb`. Existing files, allowlisted
# names and allowlisted folders pass silently.

path=$(jq -r '.tool_input.file_path // empty')
case "$path" in *.md|*.MD|*.markdown) ;; *) exit 0 ;; esac
[ -e "$path" ] && exit 0

name=$(basename "$path" | tr '[:lower:]' '[:upper:]')
case "$name" in
  README*|CHANGELOG*|CONTRIBUTING*|LICENSE*|SECURITY*|CODE_OF_CONDUCT*|CLAUDE.MD|CLAUDE.LOCAL.MD|AGENTS.MD|SKILL.MD) exit 0 ;;
esac
case "$path" in
  "$HOME"/.claude/*|*/.claude/*) exit 0 ;; # memory, plans, skills
esac
# Notes by name are asked about wherever they go, docs/ included.
case "$name" in
  PLAN.MD|PLAN[-_.]*|*[-_]PLAN.MD|PLANNING*|NOTES*|TODO*|FINDINGS*|PROGRESS*|HANDOFF*|SCRATCH*|DECISIONS*|WORKLOG*|STATUS.MD) ;;
  *)
    # A docs folder that already exists. One made just for this file isn't a docs site.
    case "$path" in
      */docs/*) [ -d "${path%%/docs/*}/docs" ] && exit 0 ;;
      */doc/*) [ -d "${path%%/doc/*}/doc" ] && exit 0 ;;
    esac ;;
esac

jq -n --arg p "$path" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "ask",
  permissionDecisionReason: ("New markdown file " + $p + ". Agent notes, plans and handoffs belong in a notebook, even under docs/: nb add <subject>.nb -i <id> -t <title> (notebook skill). Approve only if you asked for this .md file.")}}'
