// Verified rho extensions: standard MCP tool servers and lifecycle hook recipes
export const CURATED_EXTENSIONS = [
  {
    id: "mcp-filesystem",
    name: "Filesystem (MCP)",
    type: "mcp",
    badge: "MCP Server",
    category: "File Operations",
    description: "Official Model Context Protocol server for bounded local filesystem read/write operations outside the current workspace.",
    author: "modelcontextprotocol",
    version: "v1.0.2",
    runtime: "Node.js (npx)",
    repoUrl: "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
    snippet: `{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}`,
    snippetLabel: ".mcp.json / ~/.agents/mcp.json",
    isOfficial: false
  },
  {
    id: "mcp-github",
    name: "GitHub (MCP)",
    type: "mcp",
    badge: "MCP Server",
    category: "Developer Tools",
    description: "Query repository issues, review pull requests, read branches, and inspect commit history directly in agent turns.",
    author: "modelcontextprotocol",
    version: "v0.6.1",
    runtime: "Node.js (npx)",
    repoUrl: "https://github.com/modelcontextprotocol/servers/tree/main/src/github",
    snippet: `{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "\${GITHUB_TOKEN}" }
    }
  }
}`,
    snippetLabel: ".mcp.json / ~/.agents/mcp.json",
    isOfficial: false
  },
  {
    id: "mcp-playwright",
    name: "Playwright (MCP)",
    type: "mcp",
    badge: "MCP Server",
    category: "Browser Automation",
    description: "Headless browser automation via Playwright. Allows the agent to render web apps, interact with forms, and take screenshots.",
    author: "modelcontextprotocol",
    version: "v0.1.0",
    runtime: "Node.js (npx)",
    repoUrl: "https://github.com/modelcontextprotocol/servers/tree/main/src/playwright",
    snippet: `{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@playwright/mcp", "--headless"]
    }
  }
}`,
    snippetLabel: ".mcp.json / ~/.agents/mcp.json",
    isOfficial: false
  },
  {
    id: "mcp-sqlite",
    name: "SQLite (MCP)",
    type: "mcp",
    badge: "MCP Server",
    category: "Database",
    description: "Query and inspect SQLite database files with schema discovery and parameterized read-only safety guardrails.",
    author: "modelcontextprotocol",
    version: "v0.1.0",
    runtime: "Python (uvx)",
    repoUrl: "https://github.com/modelcontextprotocol/servers/tree/main/src/sqlite",
    snippet: `{
  "mcpServers": {
    "sqlite": {
      "command": "uvx",
      "args": ["mcp-server-sqlite", "--db-path", "test.db"]
    }
  }
}`,
    snippetLabel: ".mcp.json / ~/.agents/mcp.json",
    isOfficial: false
  },
  {
    id: "hook-bash-guard",
    name: "Bash Command Guard",
    type: "hook",
    badge: "Lifecycle Hook",
    category: "Security (Bash)",
    description: "One-shot shell hook in .agents/hooks/on_tool_call that blocks destructive bash commands (rm -rf, DROP TABLE, git reset --hard) with zero daemon overhead.",
    author: "casonadams",
    version: "Recipe",
    runtime: "POSIX Shell",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/hooks",
    snippet: `#!/bin/sh
# .agents/hooks/on_tool_call
read -r EVENT
if echo "$EVENT" | grep -Eq 'rm -rf|git reset --hard'; then
  echo '{"action":"stop","reason":"destructive command blocked"}'
fi`,
    snippetLabel: ".agents/hooks/on_tool_call",
    isOfficial: true
  },
  {
    id: "hook-python-audit",
    name: "Python Audit Logger",
    type: "hook",
    badge: "Lifecycle Hook",
    category: "Auditing (Python)",
    description: "One-shot Python hook in .agents/hooks/on_tool_result that appends structured tool execution logs and timestamps to an audit file.",
    author: "casonadams",
    version: "Recipe",
    runtime: "Python 3",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/hooks",
    snippet: `#!/usr/bin/env python3
# .agents/hooks/on_tool_result
import sys, json
data = json.loads(sys.stdin.read() or "{}")
with open("audit.log", "a") as f:
    f.write(f"{data.get('tool_name')}: {data.get('is_error')}\\n")
print('{"action":"continue"}')`,
    snippetLabel: ".agents/hooks/on_tool_result",
    isOfficial: true
  },
  {
    id: "hook-rtk-rewrite",
    name: "RTK Token Optimizer",
    type: "hook",
    badge: "Lifecycle Hook",
    category: "Optimization (Shell)",
    description: "One-shot shell hook in .agents/hooks/on_tool_call that uses rtk rewrite on bash commands, saving 50-90% on command output tokens.",
    author: "casonadams",
    version: "Recipe",
    runtime: "sh, jq, rtk",
    repoUrl: "https://github.com/rtk-ai/rtk",
    snippet: `#!/bin/sh
# .agents/hooks/on_tool_call (rtk >= 1.2)
event=$(cat)
cmd=$(printf '%s' "$event" | jq -r 'select(.tool_name == "bash") | .args.command // empty')
[ -n "$cmd" ] || exit 0

out=$(rtk rewrite "$cmd")
case $? in
  0|3) [ -n "$out" ] && printf '%s' "$event" | jq -c --arg c "$out" '{action: "rewrite_args", args: (.args + {command: $c})}' ;;
  2) [ -n "$out" ] && jq -nc --arg r "$out" '{action: "skip", reason: $r}' ;;
esac
exit 0`,
    snippetLabel: ".agents/hooks/on_tool_call",
    isOfficial: true
  }
];
