#!/usr/bin/env node
/**
 * Node.js TUI Block & Audit Plugin for rho
 *
 * Demonstrates:
 * - Subscribing to `tool_call` and `tool_result`
 * - Emitting rich styled TUI cards via `host/ui/block`
 * - Updating the status footer via `host/ui/set_status`
 */

const readline = require("readline");

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

let rpcCounter = 2000;
let toolExecutionCount = 0;

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    const req = JSON.parse(trimmed);
    const { id, method, params } = req;

    if (method === "initialize") {
      emit({
        jsonrpc: "2.0",
        id,
        result: {
          subscribes: ["tool_call", "tool_result"],
          serverInfo: { name: "node-tui-notifier", version: "1.0.0" }
        }
      });
    } else if (method === "hook/tool_call") {
      toolExecutionCount++;

      // 1. Update the status footer
      rpcCounter++;
      emit({
        jsonrpc: "2.0",
        id: rpcCounter,
        method: "host/ui/set_status",
        params: {
          key: "node_audit",
          text: `Tools run: ${toolExecutionCount}`
        }
      });

      // 2. Allow tool to proceed
      emit({
        jsonrpc: "2.0",
        id,
        result: { action: "continue" }
      });
    } else if (method === "hook/tool_result") {
      const toolName = params.tool_name || "unknown";
      const outputLen = (params.output || "").length;

      // 3. Render a rich styled TUI card in the transcript!
      rpcCounter++;
      emit({
        jsonrpc: "2.0",
        id: rpcCounter,
        method: "host/ui/block",
        params: {
          title: "Node.js Audit Card",
          content: `• Tool: ${toolName}\n• Output: ${outputLen} characters\n• Security Policy: Passed`,
          style: "success" // "info" | "warning" | "error" | "success"
        }
      });

      emit({
        jsonrpc: "2.0",
        id,
        result: { action: "continue" }
      });
    } else {
      emit({
        jsonrpc: "2.0",
        id,
        result: { action: "continue" }
      });
    }
  } catch (err) {
    // Ignore malformed input
  }
});

function emit(payload) {
  process.stdout.write(JSON.stringify(payload) + "\n");
}
