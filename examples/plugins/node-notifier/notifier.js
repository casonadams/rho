#!/usr/bin/env node
/**
 * Node.js Notification & Audit Plugin for rho
 */

const readline = require("readline");

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

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
      // Allow execution
      emit({
        jsonrpc: "2.0",
        id,
        result: { action: "continue" }
      });
    } else if (method === "hook/tool_result") {
      // Acknowledge observation
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
