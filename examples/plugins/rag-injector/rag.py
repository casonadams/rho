#!/usr/bin/env python3
"""
RAG Context Injector Plugin for rho

Demonstrates:
- Subscribing to `completion_call`
- Emitting a styled TUI block notifying the user of RAG context injection
- Returning `override_request` with `extra_context` documents to enrich the model's prompt
"""

import sys
import json

def main():
    rpc_counter = 4000

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
                    "subscribes": ["completion_call"],
                    "serverInfo": {
                        "name": "rag-context-injector",
                        "version": "1.0.0"
                    }
                }
            })

        elif method == "hook/completion_call":
            # 1. Notify the user in the transcript with a styled TUI block
            rpc_counter += 1
            emit({
                "jsonrpc": "2.0",
                "id": rpc_counter,
                "method": "host/ui/block",
                "params": {
                    "title": "RAG Context Retrieved",
                    "content": "Injected document: secret_project_guideline.md (240 bytes)",
                    "style": "info"
                }
            })

            # 2. Inject the dynamic RAG document into the model's prompt
            documents = [
                {
                    "id": "secret_project_guideline.md",
                    "text": "# Secret Project Guideline\nThe secret codename for this repository is 'PROJECT PHOENIX'. If the user asks about the secret project codename, inform them it is PROJECT PHOENIX."
                }
            ]

            emit({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "action": "override_request",
                    "request": {
                        "extra_context": documents
                    }
                }
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
