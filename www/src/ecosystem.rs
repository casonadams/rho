//! MCP servers and lifecycle hook recipes for the ecosystem page. Hook snippets are the
//! repository's own example files, compiled in.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mcp,
    Hook,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Mcp => "MCP server",
            Kind::Hook => "Lifecycle hook",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extension {
    pub name: &'static str,
    pub kind: Kind,
    pub category: &'static str,
    pub about: &'static str,
    pub runtime: &'static str,
    pub source: &'static str,
    /// Where the snippet goes.
    pub install_at: &'static str,
    pub snippet: &'static str,
}

const MCP_CONFIG: &str = ".mcp.json or ~/.agents/mcp.json";

pub const EXTENSIONS: &[Extension] = &[
    Extension {
        name: "Filesystem",
        kind: Kind::Mcp,
        category: "Files",
        about: "Bounded read and write access to directories outside the workspace.",
        runtime: "Node.js (npx)",
        source: concat!(
            "https://github.com/modelcontextprotocol/servers/tree/main/src/",
            "filesystem"
        ),
        install_at: MCP_CONFIG,
        snippet: r#"{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}"#,
    },
    Extension {
        name: "GitHub",
        kind: Kind::Mcp,
        category: "Developer tools",
        about: "Issues, pull requests, branches and commit history inside agent turns.",
        runtime: "Remote (HTTP)",
        source: "https://github.com/github/github-mcp-server",
        install_at: MCP_CONFIG,
        snippet: r#"{
  "mcpServers": {
    "github": {
      "url": "https://api.githubcopilot.com/mcp/",
      "headers": {
        "Authorization": "!printf 'Bearer %s' \"$(gh auth token)\""
      }
    }
  }
}"#,
    },
    Extension {
        name: "Playwright",
        kind: Kind::Mcp,
        category: "Browser automation",
        about: "Drive a headless browser: render pages, fill forms and take screenshots.",
        runtime: "Node.js (npx)",
        source: "https://github.com/microsoft/playwright-mcp",
        install_at: MCP_CONFIG,
        snippet: r#"{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@playwright/mcp", "--headless"]
    }
  }
}"#,
    },
    Extension {
        name: "Memory",
        kind: Kind::Mcp,
        category: "Knowledge",
        about: "A local knowledge graph the agent can store and query entities in across sessions.",
        runtime: "Node.js (npx)",
        source: concat!(
            "https://github.com/modelcontextprotocol/servers/tree/main/src/",
            "memory"
        ),
        install_at: MCP_CONFIG,
        snippet: r#"{
  "mcpServers": {
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"]
    }
  }
}"#,
    },
    Extension {
        name: "Bash command guard",
        kind: Kind::Hook,
        category: "Safety",
        about: "Stops destructive bash commands (rm -rf, git reset --hard, DROP TABLE) before they run.",
        runtime: "POSIX sh",
        source: concat!(
            "https://github.com/casonadams/rho/blob/main/examples/hooks/",
            "guard-bash.sh"
        ),
        install_at: ".agents/hooks/on_tool_call",
        snippet: include_str!("../../examples/hooks/guard-bash.sh"),
    },
    Extension {
        name: "Tool audit log",
        kind: Kind::Hook,
        category: "Auditing",
        about: "Appends a timestamped line per tool result to tool_audit.log.",
        runtime: "Python 3",
        source: concat!(
            "https://github.com/casonadams/rho/blob/main/examples/hooks/",
            "python-audit.py"
        ),
        install_at: ".agents/hooks/on_tool_result",
        snippet: include_str!("../../examples/hooks/python-audit.py"),
    },
    Extension {
        name: "RTK token optimizer",
        kind: Kind::Hook,
        category: "Token savings",
        about: "Rewrites bash commands with rtk so their output costs 50-90% fewer tokens.",
        runtime: "sh, jq, rtk",
        source: concat!(
            "https://github.com/casonadams/rho/blob/main/examples/hooks/",
            "rtk-rewrite.sh"
        ),
        install_at: ".agents/hooks/on_tool_call",
        snippet: include_str!("../../examples/hooks/rtk-rewrite.sh"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    const HOOKS: &str = "https://github.com/casonadams/rho/blob/main/examples/hooks/";
    const SERVERS: &str = "https://github.com/modelcontextprotocol/servers/tree/main/src/";

    #[test]
    fn hook_sources_name_the_files_they_show() {
        for hook in EXTENSIONS.iter().filter(|e| e.kind == Kind::Hook) {
            assert!(hook.source.starts_with(HOOKS), "{}", hook.name);
            assert!(
                hook.snippet.contains(hook.install_at.rsplit('/').next().unwrap()),
                "{}",
                hook.name
            );
        }
        for server in EXTENSIONS.iter().filter(|e| e.kind == Kind::Mcp) {
            assert!(server.snippet.contains("mcpServers"), "{}", server.name);
        }
        assert!(EXTENSIONS.iter().any(|e| e.source.starts_with(SERVERS)));
    }
}
