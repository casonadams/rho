// Verified rho extensions, MCP tool servers, example plugins, and SDK packages
export const CURATED_PLUGINS = [
  {
    id: "rho-plugin-sdk",
    name: "rho-plugin-sdk",
    type: "sdk",
    badge: "Official SDK",
    category: "Rust Crate",
    description: "Official Rust SDK for developing rho JSON-RPC daemon plugins. Subscribe to tool_call and tool_result, repair invalid tools, and invoke host UI modals.",
    author: "casonadams",
    version: "v0.6.0",
    runtime: "Rust Crate",
    repoUrl: "https://github.com/casonadams/rho/tree/main/crates/rho-plugin-sdk",
    cratesUrl: "https://crates.io/crates/rho-plugin-sdk",
    snippet: "cargo add rho-plugin-sdk",
    snippetLabel: "cargo add rho-plugin-sdk",
    isOfficial: true
  },
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
    snippet: `[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
enabled = true`,
    snippetLabel: "config.toml snippet",
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
    snippet: `[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_..." }
enabled = true`,
    snippetLabel: "config.toml snippet",
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
    snippet: `[mcp.servers.playwright]
command = "npx"
args = ["-y", "@playwright/mcp", "--headless"]
enabled = true`,
    snippetLabel: "config.toml snippet",
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
    snippet: `[mcp.servers.sqlite]
command = "uvx"
args = ["mcp-server-sqlite", "--db-path", "test.db"]
enabled = true`,
    snippetLabel: "config.toml snippet",
    isOfficial: false
  },
  {
    id: "plugin-rust-guard",
    name: "rust-guard",
    type: "plugin",
    badge: "Example Daemon",
    category: "Security (Rust)",
    description: "Rust daemon plugin built with rho-plugin-sdk. Intercepts bash commands, triggers interactive Yes/No confirmation modals via Host UI, and logs audit reports.",
    author: "casonadams",
    version: "Example",
    runtime: "Rust (compiled binary)",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/plugins/rust-guard",
    cratesUrl: null,
    snippet: `[plugins.rust_guard]
enabled = true
command = "rust-guard" # or path = "target/release/rust-guard"`,
    snippetLabel: "config.toml snippet",
    isOfficial: true
  },
  {
    id: "plugin-python-guard",
    name: "python-guard",
    type: "plugin",
    badge: "Example Daemon",
    category: "Security (Python)",
    description: "Interactive Python guard plugin. Demonstrates how any programming language can intercept tool calls and display approval modals via JSON-RPC 2.0.",
    author: "casonadams",
    version: "Example",
    runtime: "Python 3",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/plugins/python-guard",
    cratesUrl: null,
    snippet: `[plugins.python_guard]
enabled = true
command = "python3"
args = ["examples/plugins/python-guard/guard.py"]`,
    snippetLabel: "config.toml snippet",
    isOfficial: true
  },
  {
    id: "plugin-quota-tracker",
    name: "quota-tracker",
    type: "plugin",
    badge: "Example Daemon",
    category: "Telemetry (Node.js)",
    description: "Node.js daemon demonstrating how provider plugins surface rolling quota cooldowns (5h and 7d) in rho's interactive footer status line.",
    author: "casonadams",
    version: "Example",
    runtime: "Node.js",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/plugins/quota-tracker",
    cratesUrl: null,
    snippet: `[plugins.quota_tracker]
enabled = true
command = "node"
args = ["examples/plugins/quota-tracker/tracker.js"]`,
    snippetLabel: "config.toml snippet",
    isOfficial: true
  },
  {
    id: "plugin-rag-injector",
    name: "rag-injector",
    type: "plugin",
    badge: "Example Daemon",
    category: "Context (Python)",
    description: "Demonstrates how external plugins can query embeddings/vector databases and dynamically inject extra_context documents into agent completion prompts.",
    author: "casonadams",
    version: "Example",
    runtime: "Python 3",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/plugins/rag-injector",
    cratesUrl: null,
    snippet: `[plugins.rag_injector]
enabled = true
command = "python3"
args = ["examples/plugins/rag-injector/rag.py"]`,
    snippetLabel: "config.toml snippet",
    isOfficial: true
  },
  {
    id: "plugin-node-notifier",
    name: "node-notifier",
    type: "plugin",
    badge: "Example Daemon",
    category: "Notifications (Node.js)",
    description: "Subscribes to tool_call and tool_result lifecycle events, emitting live notification messages into the transcript via host/ui/notify.",
    author: "casonadams",
    version: "Example",
    runtime: "Node.js",
    repoUrl: "https://github.com/casonadams/rho/tree/main/examples/plugins/node-notifier",
    cratesUrl: null,
    snippet: `[plugins.node_notifier]
enabled = true
command = "node"
args = ["examples/plugins/node-notifier/notifier.js"]`,
    snippetLabel: "config.toml snippet",
    isOfficial: true
  }
];
