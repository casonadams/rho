#!/usr/bin/env python3
# Hook for .agents/hooks/on_tool_call
# Automatically optimizes bash commands using RTK (https://github.com/rtk-ai/rtk).
import json
import subprocess
import sys


def rewrite_command(cmd):
    if not cmd or cmd.startswith("rtk "):
        return None
    res = subprocess.run(["rtk", "rewrite", cmd], capture_output=True, text=True)
    rewritten = res.stdout.strip()
    if res.returncode in (0, 3) and rewritten and rewritten != cmd:
        return rewritten
    return None


def main():
    try:
        event = json.load(sys.stdin)
        if event.get("tool_name") != "bash":
            return

        args = event.get("args") or {}
        cmd = args.get("command", "")
        rewritten = rewrite_command(cmd)
        if rewritten:
            args["command"] = rewritten
            print(json.dumps({"action": "rewrite_args", "args": args}))
    except Exception:
        pass


if __name__ == "__main__":
    main()
