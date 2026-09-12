#!/usr/bin/env python3
# Example hook: place in .rho/hooks/on_tool_result and chmod +x
import sys
import json
from datetime import datetime

try:
    raw = sys.stdin.read().strip()
    if raw:
        event = json.loads(raw)
        tool_name = event.get("tool_name", "unknown")
        is_error = event.get("is_error", False)
        status = "ERROR" if is_error else "OK"
        log_entry = f"{datetime.now().isoformat()} [{status}] tool={tool_name}\n"
        with open("tool_audit.log", "a") as f:
            f.write(log_entry)
except Exception:
    pass

# Keep the original result
print(json.dumps({"action": "continue"}))
