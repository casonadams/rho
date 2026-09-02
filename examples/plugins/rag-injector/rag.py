#!/usr/bin/env python3
"""
RAG Context Injector Plugin for rho

Demonstrates:
- Subscribing to `completion_call`
- Returning `override_request` with `extra_context` documents to enrich the model's prompt
"""

import sys
import json

def main():
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
            params = req.get("params", {})
            prompt = params.get("prompt", {})

            # In a full RAG plugin, embed prompt and query a vector store.
            # Here we inject relevant documentation context:
            documents = [
                {
                    "id": "architecture_guide.md",
                    "text": "# Architecture Guidelines\n- Always use domain models for business logic.\n- Keep adapters thin and free of state."
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
