#!/usr/bin/env node
/**
 * Quota Tracker Plugin for rho
 *
 * Demonstrates:
 * - Subscribing to `completion_response`
 * - Tracking 5h and 7d rolling quota cooldowns (matching ../status-line)
 * - Dynamically updating rho's footer via `host/ui/set_status`
 */

const readline = require("readline");

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

let rpcCounter = 3000;
let requestCount = 0;

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    const req = JSON.parse(trimmed);
    const { id, method } = req;

    if (method === "initialize") {
      emit({
        jsonrpc: "2.0",
        id,
        result: {
          subscribes: ["completion_response"],
          serverInfo: { name: "quota-tracker", version: "1.0.0" }
        }
      });
    } else if (method === "hook/completion_response") {
      requestCount++;

      // Simulate rolling cooldown window calculations
      const fiveHourUsage = Math.min(100, 45 + requestCount * 5);
      const sevenDayUsage = Math.min(100, 15 + requestCount * 2);
      const statusText = `5h: ${fiveHourUsage}% • 7d: ${sevenDayUsage}%`;

      // Update the footer slot
      rpcCounter++;
      emit({
        jsonrpc: "2.0",
        id: rpcCounter,
        method: "host/ui/set_status",
        params: {
          key: "quota",
          text: statusText
        }
      });

      // Acknowledge completion response
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
