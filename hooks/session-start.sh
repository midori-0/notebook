#!/bin/sh
# Claude Code SessionStart hook. While notebook mode is on, gives each session the rule to keep
# its own notes in `nb`. Says nothing when it's off.

skill="$(dirname "$0")/../skills/notebook"
[ "$("$skill/mode.sh")" = on ] || exit 0
jq -n --rawfile rule "$skill/rule.txt" '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $rule}}'
