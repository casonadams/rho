# rho

`rho` is a fast, clean, minimal coding-agent CLI built in Rust on [Rig 0.42](https://github.com/0xPlaygrounds/rig).

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

- **`read`**: Read file contents with line numbering, offset, and limit safeguards. Supports image sniffing and downscaling.
- **`write`**: Create or overwrite files, automatically creating missing parent directories.
- **`edit`**: Apply targeted, exact text replacements to files.
- **`bash`**: Execute shell commands with process group cleanup, timeout protection, and binary sanitization.
- **`fd`**: Fast, gitignore-aware workspace file discovery with smart-case regex matching.
- **`rg`**: Fast, line-oriented content searching; gitignore-aware, skips binary files, and bounds output.
- **`web_search`**: Search the web and retrieve structured summaries and URLs.
- **`web_fetch`**: Fetch and extract clean markdown, text, HTML, CSV, or feeds from web URLs.

---

## Feature Highlights & Documentation

Comprehensive guides are organized in [`docs/`](docs/):

- **[Providers, Configuration & Skills](docs/configuration.md)**
  - Authentication for 14+ providers (ChatGPT, Claude, Copilot, Antigravity, Anthropic, Gemini, DeepSeek, Local Ollama, and more).
  - Custom OpenAI-compatible endpoints with private network protection.
  - Hierarchical instruction loading (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`).
  - Declarative parameter-guided skills (`SKILL.md`).

- **[Interactive UI & Theming](docs/ui.md)**
  - Dynamic upward-expanding multiline editor with stable screen scrollback.
  - Live two-line footer telemetry: token count, context percentage, speed, cost, and active model.
  - In-memory FIFO message queueing (`Alt+Enter`) and non-blocking background turns.
  - 10 built-in color themes, live selector (`/theme`), and dynamic [walh-shell](https://github.com/casonadams/walh-shell) ANSI syncing.
  - Fenced Mermaid diagram rendering.

- **[Permissions & Safety](docs/permissions.md)**
  - In-process safety layer separating baseline safe inspection from mutating commands.
  - Interactive approval modals: **Allow**, **Edit** (with multiline arrow navigation), **Always** (with pattern matching), and **Deny** (with feedback).
  - Fail-closed execution in headless automation.

- **[MCP Servers & Lifecycle Plugins](docs/plugins.md)**
  - Standard Model Context Protocol (MCP) server integration for external tools.
  - JSON-RPC stdio daemon plugin architecture for custom lifecycle steering and guardrails.
  - Native Rust plugin development via [`rho-plugin-sdk`](https://crates.io/crates/rho-plugin-sdk).
  - Built-in plugin package management (`rho install`, `rho update`, `rho remove`).

---

## Development

Run the test suite, linter, and format checks:

```sh
cargo test --workspace
make clippy
cargo fmt --all -- --check
```
