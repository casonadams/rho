#!/usr/bin/env node
/**
 * Node.js Notification & Audit Plugin for rho
 *
 * Demonstrates:
 * - Subscribing to `tool_call` and `tool_result`
 * - Calling `host/ui/notify` to display real-time notices in rho's transcript
 */

const readline = require("readline");

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

let rpcCounter = 2000;

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
          serverInfo: { name: "node-notifier", version: "1.0.0" }
        }
      });
    } else if (method === "hook/tool_call") {
      const toolName = params.tool_name || "unknown";

      // 1. Emit a notification into rho's transcript
      rpcCounter++;
      emit({
        jsonrpc: "2.0",
        id: rpcCounter,
        method: "host/ui/notify",
        params: {
          message: `[Node.js Notifier] Starting tool '${toolName}'...`,
          level: "info"
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

      // Emit a completion notification into rho's transcript
      rpcCounter++;
      emit({
        jsonrpc: "2.0",
        id: rpcCounter,
        method: "host/ui/notify",
        params: {
          message: `[Node.js Notifier] Finished '${toolName}' (${outputLen} chars output)`,
          level: "info"
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
