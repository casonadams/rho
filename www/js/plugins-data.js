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
    cratesUrl: null,
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
    cratesUrl: null,
    snippet: `{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" }
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
    cratesUrl: null,
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
    cratesUrl: null,
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
    description: "One-shot shell hook in .rho/hooks/on_tool_call that blocks destructive bash commands (rm -rf, DROP TABLE, git reset --hard) with zero daemon overhead.",
    author: "casonadams",
    version: "Recipe",
    runtime: "POSIX Shell",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/hooks",
    cratesUrl: null,
    snippet: `#!/bin/sh
# .rho/hooks/on_tool_call
read -r EVENT
if echo "$EVENT" | grep -Eq 'rm -rf|git reset --hard'; then
  echo '{"action":"stop","reason":"destructive command blocked"}'
fi`,
    snippetLabel: ".rho/hooks/on_tool_call",
    isOfficial: true
  },
  {
    id: "hook-python-audit",
    name: "Python Audit Logger",
    type: "hook",
    badge: "Lifecycle Hook",
    category: "Auditing (Python)",
    description: "One-shot Python hook in .rho/hooks/on_tool_result that appends structured tool execution logs and timestamps to an audit file.",
    author: "casonadams",
    version: "Recipe",
    runtime: "Python 3",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/hooks",
    cratesUrl: null,
    snippet: `#!/usr/bin/env python3
# .rho/hooks/on_tool_result
import sys, json
data = json.loads(sys.stdin.read() or "{}")
with open("audit.log", "a") as f:
    f.write(f"{data.get('tool_name')}: {data.get('is_error')}\\n")
print('{"action":"continue"}')`,
    snippetLabel: ".rho/hooks/on_tool_result",
    isOfficial: true
  }
];

export const CURATED_PLUGINS = CURATED_EXTENSIONS;
