# rho

`rho` is a fast, clean, minimal coding-agent CLI built in Rust on
[Rig](https://github.com/0xPlaygrounds/rig).

---

## Installation

```sh
cargo install rho
```

---

## Quick Start

```sh
# Start interactive REPL
rho

# Run one-shot prompt
rho -p "summarize this repository"

# Select provider and model
rho --provider gemini --model gemini-2.5-flash
rho --provider anthropic --model claude-3-7-sonnet-latest

# Resume a previous session
rho --resume <SESSION_ID>
# Or browse recent sessions interactively
rho --resume-picker
```

---

## Built-in Tools

`rho` includes 8 fast, native tools designed for coding agents:

- **`read`**: Read file contents with line numbering, offset, and limit
  safeguards. Supports image sniffing and downscaling.
- **`write`**: Create or overwrite files, automatically creating missing parent
  directories.
- **`edit`**: Apply targeted, exact text replacements to files.
- **`bash`**: Execute shell commands with process group cleanup, timeout
  protection, and binary sanitization.
- **`fd`**: Fast, gitignore-aware workspace file discovery with smart-case regex
  matching.
- **`rg`**: Fast, line-oriented content searching; gitignore-aware, skips binary
  files, and bounds output.
- **`web_search`**: Search the web and retrieve structured summaries and URLs.
- **`web_fetch`**: Fetch and extract clean markdown, text, HTML, CSV, or feeds
  from web URLs.

---

## Feature Highlights & Documentation

Comprehensive guides are organized in [`docs/`](docs/):

- **[Providers, Configuration & Skills](docs/configuration.md)**
  - Authentication for 14+ providers (ChatGPT, Claude, Copilot, Antigravity,
    Anthropic, Gemini, DeepSeek, Local Ollama, and more).
  - Custom OpenAI-compatible endpoints with private network protection.
  - Hierarchical instruction loading (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`).
  - Declarative parameter-guided skills (`SKILL.md`).

- **[Interactive UI & Terminal Styling](docs/ui.md)**
  - Dynamic upward-expanding multiline editor with stable screen scrollback.
  - Live two-line footer metrics: token count, context percentage, speed,
    cost, and active model.
  - In-memory FIFO message queueing (`Alt+Enter`) and non-blocking background
    turns.
  - Universal native theme that adopts your terminal's 16-color palette, with
    SGR-dim secondary text, terminal-detected card fills, or configurable outline borders.
  - Fenced Mermaid diagram rendering.

- **[Keyboard Shortcuts & Controls](docs/shortcuts.md)**
  - Comprehensive keybinding reference organized by session flow, queueing,
    model controls, and editor navigation.
  - Custom keybinding configuration (`~/.config/rho/keybindings.toml`).

- **[Permissions, Privacy & Safety](docs/permissions.md)**
  - Strict zero-telemetry policy: rho collects nothing, no analytics, no phone-home pings.
  - In-process safety layer separating baseline safe inspection from mutating
    commands.
  - Interactive approval modals: **Allow**, **Edit** (with multiline arrow
    navigation), **Always** (with pattern matching), and **Deny** (with
    feedback).
  - Fail-closed execution in headless automation.

- **[MCP Servers & Lifecycle Plugins](docs/plugins.md)**
  - Full Model Context Protocol (MCP) support: `stdio` and `streamable-http` transports with OAuth 2.1 PKCE authorization.
  - Workspace `.mcp.json` interoperability, MCP resources, prompts, and root boundary negotiation.
  - Context budget tool gating (`direct` vs `gateway` vs `auto`), `/mcp` TUI modal, and `rho mcp` management suite.
  - JSON-RPC stdio daemon plugin architecture for custom lifecycle steering and
    guardrails.
  - Native Rust plugin development via
    [`rho-plugin-sdk`](https://crates.io/crates/rho-plugin-sdk).
  - Built-in plugin package management (`rho install`, `rho update`,
    `rho remove`).

---

## Privacy & Zero Telemetry

`rho` is built from the ground up with a strict privacy-first architecture:

- **Zero Telemetry**: `rho` collects **nothing**. There are no analytics, no telemetry, no tracking pixels, no crash reporters, and no phone-home pings.
- **Direct-to-Provider**: Network requests travel strictly and directly between your machine and your configured model provider (e.g. Anthropic, OpenAI, Gemini, local Ollama). No prompts, source code, or completions are ever routed through intermediary proxies or secondary servers.
- **100% Local Storage**: All authentication credentials, session histories, transcripts, and context caches reside exclusively on your local filesystem (in `~/.config/rho`, `~/.local/share/rho`, and project `.rho/`).
- **Open Source & Auditable**: The entire codebase is open source under MIT / Apache-2.0 and contains zero analytics SDKs or tracking dependencies.

---

## Development

Run the test suite, linter, and format checks:

```sh
cargo test --workspace
make clippy
cargo fmt --all -- --check
```
