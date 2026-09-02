#!/usr/bin/env python3
"""
Python Security Guard Plugin for rho

Demonstrates:
- Subscribing to `hook/tool_call`
- Requesting interactive confirmation via `host/ui/confirm`
- Returning Rig `Flow` actions (`continue` or `skip`)
"""

import sys
import json

def main():
    for line in sys.stdin:
        line = line.trim() if hasattr(line, "trim") else line.strip()
        if not line:
            continue

        try:
            req = json.loads(line)
        except Exception:
            continue

        method = req.get("method")
        req_id = req.get("id")

        if method == "initialize":
            # Declare event subscriptions
            response = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "subscribes": ["tool_call", "invalid_tool_call"],
                    "serverInfo": {
                        "name": "python-security-guard",
                        "version": "1.0.0"
                    }
                }
            }
            emit(response)

        elif method == "hook/tool_call":
            params = req.get("params", {})
            tool_name = params.get("tool_name", "")
            args = params.get("args", {})

            # 1. Unconditionally block dangerous root deletions
            if tool_name == "bash" and "rm -rf /" in args.get("command", ""):
                emit({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "action": "skip",
                        "reason": "Permission denied: destructive root deletion is blocked by policy."
                    }
                })
                continue

            # 2. Ask user for confirmation on sensitive commands (e.g. sudo, reboot)
            if tool_name == "bash" and any(k in args.get("command", "") for k in ["sudo", "reboot", "shutdown"]):
                cmd = args.get("command", "")
                # Request interactive confirmation from rho's TUI
                prompt_id = 999
                confirm_req = {
                    "jsonrpc": "2.0",
                    "id": prompt_id,
                    "method": "host/ui/confirm",
                    "params": {
                        "title": "Privileged Execution Request",
                        "message": f"Allow executing privileged command: {cmd}?"
                    }
                }
                emit(confirm_req)

                # Await host reply
                confirmed = False
                for reply_line in sys.stdin:
                    try:
                        reply = json.loads(reply_line.strip())
                        if reply.get("id") == prompt_id:
                            confirmed = reply.get("result", {}).get("confirmed", False)
                            break
                    except Exception:
                        break

                if confirmed:
                    emit({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {"action": "continue"}
                    })
                else:
                    emit({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "action": "skip",
                            "reason": "Permission denied by user. Do not retry this operation."
                        }
                    })
                continue

            # 3. Allow all other tools
            emit({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"action": "continue"}
            })

        elif method == "hook/invalid_tool_call":
            params = req.get("params", {})
            tool_name = params.get("tool_name", "")

            # Auto-repair common hallucinated tool names
            if tool_name in ["sh", "shell", "terminal"]:
                emit({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "action": "repair",
                        "tool_name": "bash"
                    }
                })
            else:
                emit({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"action": "continue"}
                })
        else:
            emit({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"action": "continue"}
            })

def emit(payload):
    print(json.dumps(payload), flush=True)

if __name__ == "__main__":
    main()
