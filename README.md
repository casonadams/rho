# rho

`rho` is a fast, clean, local, private, and secure coding agent CLI built in Rust.

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

# Select model in <provider>/<model> format
rho --model anthropic/claude-3-7-sonnet
rho --model gemini/gemini-2.5-flash

# Resume a previous session
rho --resume <SESSION_ID>
# Or browse recent sessions interactively
rho --resume-picker
```

When a session that has content ends, rho prints `Resume this session: rho --resume <SESSION_ID>`
to stderr. In `/resume` and `--resume-picker`, each row is titled by the session name (see `/name`) or its first message,
with the age, turn count and session ID beside it.

---

## Built-in Tools

`rho` includes 10 fast, native tools designed for coding agents:

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
- **`subagent`**: Spawn isolated subagents with specialized roles (`scout`, `critic`,
  `planner`, `general`) to perform focused research, trade-off analysis, or code review
  without parent context bloat. Subagent tool calls go through the same permission
  rules and lifecycle hooks as the parent, stop on cancellation, stream per-turn
  progress, and return a summary with turn, tool-call, and token counts. An unknown
  `role` or `agent` name is an error. A custom agent's `model` field selects its model
  (`inherit`, `sonnet`, `opus` and `haiku` use the parent's); a model that cannot be
  resolved is an error. `inherit_context: true` shares the recent text-only parent
  conversation with the subagent (and its model provider). Several subagent calls in one
  turn run concurrently when every tool they can use is read-only; permission prompts
  still appear one at a time.
- **`web_search`**: Search the web and retrieve structured summaries and URLs. Supports selectable search engines (Brave, DuckDuckGo Lite, Yahoo).
- **`web_fetch`**: Fetch and extract clean markdown, text, HTML, CSV, or feeds
  from web URLs.

Web tools, search providers, MCP, and permissions can be configured in `config.toml` or interactively toggled via `/settings` under **Tools & Permissions**.

---

## Feature Highlights & Documentation

Comprehensive guides are organized in [`docs/`](docs/):

- **[Providers, Configuration & Skills](docs/configuration.md)**
  - Authentication for 13+ providers (ChatGPT, Claude, Antigravity,
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

- **[Keyboard Shortcuts & Controls](docs/shortcuts.md)**
  - Comprehensive keybinding reference organized by session flow, queueing,
    model controls, and editor navigation.

- **[Permissions, Privacy & Safety](docs/permissions.md)**
  - Strict zero-telemetry policy: rho collects nothing, no analytics, no phone-home pings.
  - In-process safety layer separating baseline safe inspection from mutating commands.
  - Delegated **Guard Model** classification: automatically classifies shell commands using a fast local model (`qwen2.5-coder:7b` via Ollama) to execute safe commands without friction while intercepting hazardous operations.
  - Interactive approval modals: **Allow**, **Edit** (with multiline arrow navigation), **Always** (with pattern matching), and **Deny** (with feedback).
  - Fail-closed execution in headless automation and on guard model errors or timeouts.

- **[MCP Servers & Lifecycle Hooks](docs/mcp-and-hooks.md)**
  - Full Model Context Protocol (MCP) support: `stdio` and `streamable-http` transports with static request headers.
  - Ecosystem interoperability with `~/.agents/mcp.json`, project `.agents/mcp.json`, and `.mcp.json`.
  - Context budget tool gating (`direct` vs `gateway` vs `auto`), `/mcp` TUI modal, and `rho mcp` management suite.
  - One-shot lifecycle hooks in `.agents/hooks/` for deterministic policy enforcement, tool gating, argument rewriting, and turn control without background daemons.

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

---

## Acknowledgments

- Special thanks to [pi.dev](https://pi.dev) for inspiring aspects of rho's design and ergonomics.
