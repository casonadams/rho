#!/usr/bin/env python3
"""
Interactive Python Guard Plugin for rho

Prompts for user approval before executing ANY tool call.
"""

import sys
import json

def get_tool_summary(tool_name, args):
    if not isinstance(args, dict):
        return str(args)
    for key in ["command", "path", "url", "query"]:
        if key in args:
            return f"{key}='{args[key]}'"
    return json.dumps(args, separators=(",", ":"))

def main():
    prompt_counter = 1000

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            req = json.loads(line)
        except Exception:
            continue

        method = req.get("method")
        req_id = req.get("id")

        if method == "initialize":
            emit({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "subscribes": ["tool_call", "invalid_tool_call"],
                    "serverInfo": {
                        "name": "python-interactive-guard",
                        "version": "1.0.0"
                    }
                }
            })

        elif method == "hook/tool_call":
            params = req.get("params", {})
            tool_name = params.get("tool_name", "")
            args = params.get("args", {})
            summary = get_tool_summary(tool_name, args)

            # Send interactive confirmation request to rho
            prompt_counter += 1
            prompt_id = prompt_counter

            emit({
                "jsonrpc": "2.0",
                "id": prompt_id,
                "method": "host/ui/confirm",
                "params": {
                    "title": f"Permission Gate: {tool_name}",
                    "message": f"Execute [{tool_name}] with {summary}?",
                    "default_yes": True
                }
            })

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
                        "reason": f"Permission denied by user for '{tool_name}' execution. Do not retry this operation."
                    }
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
