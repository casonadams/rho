#!/bin/sh
# Example hook: place in .agents/hooks/on_tool_call and chmod +x
# Needs jq and rtk >= 1.2. Exit codes of `rtk rewrite`: 0 and 3 print a rewrite, 2 with output is a block message.
cmd=$(jq -r 'select(.tool_name == "bash") | .args.command // empty')
[ -n "$cmd" ] || exit 0

out=$(rtk rewrite "$cmd")
case $? in
  0|3) [ -n "$out" ] && jq -nc --arg c "$out" '{action: "rewrite_args", args: {command: $c}}' ;;
  2) [ -n "$out" ] && jq -nc --arg r "$out" '{action: "skip", reason: $r}' ;;
esac
exit 0
