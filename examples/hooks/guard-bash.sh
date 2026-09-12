#!/bin/sh
# Example hook: place in .rho/hooks/on_tool_call and chmod +x
read -r EVENT

# Check if the tool is bash and contains dangerous patterns
if echo "$EVENT" | grep -q '"tool_name":"bash"'; then
  if echo "$EVENT" | grep -Eq 'rm -rf|git reset --hard|DROP TABLE'; then
    echo '{"action": "stop", "reason": "Destructive command blocked by local hook"}'
    exit 0
  fi
fi

# Allow all other tools to proceed
exit 0
