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
# Start interactive REPL (no model is selected until you pick one with /model)
rho

# Run one-shot prompt (needs a saved default or --model)
rho -p "summarize this repository"

# Select model in <provider>/<model> format
rho --model anthropic/claude-3-7-sonnet
rho --model gemini/gemini-2.5-flash

# Resume a previous session
rho --resume <SESSION_ID>
# Or browse recent sessions interactively
rho --resume-picker
```

When a session that has content ends, rho prints `Resume this session:` followed by
`rho --resume <SESSION_ID>` on its own line to stderr. In `/resume` and `--resume-picker`, each row is titled by the session name (see `/name`) or its first message,
with the age, turn count and session ID beside it.

Session files (JSONL, one record per line) use format version 3, which stores each step of a turn
once: when the turn completes, a small `checkpoint_node` record makes its saved steps a node of the
conversation tree instead of copying them. Resuming a version 2 file upgrades it to version 3: rho
rewrites its header line (replacing the file atomically) and keeps every earlier record as it was.
An older rho refuses a version 3 file with `unsupported session version 3` rather than misreading it.

In an interactive terminal, rho also writes the current session ID to
`~/.config/rho/terminal-sessions/<tty>` (for example `ttys004`, or `pts-3` for `/dev/pts/3` on
Linux; `$RHO_HOME` replaces
`~/.config/rho`). The file is rewritten when you switch sessions, so a terminal multiplexer can
resume the exact session of each pane with `rho --resume <id>`.

---

## Built-in Tools

`rho` includes 16 fast, native tools designed for coding agents:

- **`read`**: Read file contents with line numbering, offset, and limit
  safeguards. Supports image sniffing and downscaling. `symbol` reads just one
  function, type, or method, located by the file's language server.
- **`write`**: Create or overwrite files, automatically creating missing parent
  directories.
- **`edit`**: Apply exact text replacements to files: a unique `oldText`, every match with `replaceAll`, or a whole block from `startText` through `endText`. The result lists each changed region, numbered like `read`.
- **`undo`**: Revert the last `edit`, `write`, or `lsp` rename, code action, or format to a file (up to 20 steps per
  file; a file `write` created is deleted). It refuses when the file changed
  after that tool wrote it, so it never discards changes it did not make.
- **`lsp`**: Ask the file's [language server](docs/configuration.md#language-servers)
  for diagnostics, definitions, references, hover, implementations, symbols,
  quick fixes (`code_actions`), formatting, or a workspace-wide rename. Changes
  from rename, code actions and formatting go through the same protected-path
  checks as `edit` and are all-or-nothing and undoable. `edit` and `write`
  results end with the server's errors and warnings for the new content, and
  diagnostics that arrive later are delivered as a notice.
- **`bash`**: Execute shell commands with process group cleanup, timeout
  protection, and binary sanitization. Commands run with color enabled so you
  see it in the terminal; the model receives the same output without escape
  codes. Long output is clipped to its first and last lines (more of the end
  when the command fails) with a path to the full log; when the clipped output
  has failing tests (cargo test, nextest, pytest, go test, Jest, Vitest), a
  short section lists each failing test with its key failure lines.

  Interactive programs (vim, REPLs) can be driven from `bash` through mpx:
  start one with `mpx run --pty`, type with `send-keys`, read the screen with
  `capture-pane -p -S -N`, and stop it with `kill`. With `tools.bash.direnv = true`,
  `bash` loads the environment of an allowed `.envrc` above the workspace via
  `direnv export json` before each command.
- **`fd`**: Fast, gitignore-aware workspace file discovery with smart-case regex
  matching. Entries are grouped under their parent directory (`dir/` header,
  indented names; directories end with `/`) after a total count. A directory
  whose entries are listed appears only as their header.
- **`rg`**: Fast, line-oriented content searching; gitignore-aware, skips binary
  files. Matches are grouped by file (path line, then `line:text` rows) after an
  `N matches in M files` total; `output_mode` `files` or `count` lists only
  matching files or per-file counts. `context` (0-10) adds `line-text` rows
  around each match, and `multiline` lets a pattern span lines. Both tools fill a 20KB page and name the
  exact `offset` for the next page, so every result stays reachable.
- **`subagent`**: Delegate focused work to an isolated agent: a built-in role (`scout`,
  `critic`, `planner`, `general`) or your own [custom agent](docs/configuration.md#subagents--custom-agents),
  optionally with a [persona](docs/configuration.md#personas) that changes how it behaves.
  Each subagent works in its own git worktree, with language-server queries (`lsp`)
  against that worktree; only `general` can rename.
- **`worktree`**: Review, merge (three-way, leaving conflict markers to fix), or discard the
  worktrees where subagents and workflows did their work. `diff` opens with, and `merge` ends with, a
  static review of the added code (smells, clean-code and duplication findings with
  `file:line`); it never blocks a merge and does not run tests or linters.
- **`memory`**: Keep short notes across sessions (`view`, `add`, `remove`), for
  durable, non-obvious facts such as "fidelity tests are slow; run X first".
  Notes are plain markdown bullets in `~/.agents/memory/<project key>.md`
  (scope `project`, the default) and `~/.agents/memory/global.md` (scope
  `global`), loaded into context at the start of each session; changes take
  effect in the next session. See [Memory](docs/configuration.md#memory-notes).
- **`web_search`**: Search the web and retrieve structured summaries and URLs. Supports selectable search engines (Brave, DuckDuckGo Lite, Yahoo).
- **`web_fetch`**: Fetch and extract clean markdown, text, HTML, CSV, or feeds
  from web URLs.
- **`todo`**: Keep an ordered task list (`pending`, `in_progress`, `completed`)
  for multi-step work. Each call replaces the whole list and allows at most one
  item in progress; a call without `todos` shows the current list. The list is
  read back from the conversation, so it survives `--resume`, and compaction
  copies an unfinished list into its summary. Each subagent keeps its own list.
- **`ask`**: Ask the user 1 to 5 clarifying questions in an interactive A2UI
  modal (up to 4 structured choices with optional `multi_select` and custom
  "Other" input, or free-text). Available in interactive TUI sessions only
  (omitted in headless runs and subagents).
- **`wait`**: Block until background work reports back, instead of polling with
  `sleep`. It returns as soon as a background `bash` job or subagent task
  finishes, with every result that has arrived (which is then not delivered
  again as a follow-up message), when a new message arrives, or after 10 minutes
  with a `Still running:` list. With nothing running it returns at once. Calling
  it again waits for the work still running. The footer shows it while it
  waits, and it leaves no card in the transcript. `Esc` ends the wait without
  stopping the jobs. `bash` refuses a bare `sleep`, `pgrep`, `ps`, or `tail -f`
  poll run in the foreground while a job or subagent task is running and points
  to `wait`.

When `read`, `edit`, `fd`, or `rg` gets a path that does not exist, the error
ends with up to three close workspace paths (`Did you mean: ...?`), matching
moved files, typos, and `foo.rs` versus `foo/mod.rs`.

When every tool call in one model response is read-only (`read`, `fd`, `rg`,
`web_search`, `web_fetch`, or read-only subagents), the calls run in parallel.
In a git repository, every subagent has its own worktree, so a response's
subagent calls all run in parallel. A subagent starts in the background: the
call returns its task id at once, the main agent keeps working (or calls
`wait`), can send it more instructions with its `task_id`, and gets its result
as a follow-up message. The card tells you to press the left arrow on an empty prompt to
watch it.

`rho stats [--last N]` reports per-tool call and error counts, the most common
errors, and file edits made through bash (`sed -i`, scripts, redirects) instead
of `edit`/`write`, from the sessions stored on this machine.

Web tools, search providers, MCP, and permissions can be configured in `config.toml` or interactively toggled via `/settings` under **Tools & Permissions**.

---

## Feature Highlights & Documentation

Comprehensive guides are organized in [`docs/`](docs/):

- **[Providers, Configuration & Skills](docs/configuration.md)**
  - Authentication for 13+ providers (ChatGPT, Claude, Antigravity,
    Anthropic, Gemini, DeepSeek, Local Ollama, and more).
  - Live model lists and context windows fetched from each provider every
    session, so new models need no rho update.
  - Custom OpenAI-compatible endpoints with private network protection.
  - Hierarchical instruction loading (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`).
  - Declarative parameter-guided skills (`SKILL.md`).
  - Extensible `[models.<role>]` configuration (`default`, built-in roles `smol`, `slow`, `guard`, `judge`, `widget`, `review`, `plan`, `task`, `vision`, and custom roles) with `@<role>` selector syntax.
  - Custom subagents with their own tools, model, and turn limit.

- **[Interactive UI & Terminal Styling](docs/ui.md)**
  - Dynamic upward-expanding multiline editor with stable screen scrollback.
  - Live two-line footer metrics: token count, context percentage, speed,
    cost, and active model.
  - Opt-in A2UI v1.0 persistent TUI widgets (`[ui] a2ui_widgets = true`, `rho:todo`, `rho:subagents`) and interactive A2UI modals (`ask` tool with optional `decision_assist`).
  - In-memory FIFO message queueing (`Alt+Enter`) and non-blocking background
    turns.
  - Ephemeral mid-turn side questions (`/btw`), idle/resume session recaps
    (`/recap`), multiplexer session forking (`/fork pane|window|tab`), and
    adaptive per-turn reasoning (`thinking_level = "auto"`).
  - Universal native theme that adopts your terminal's 16-color palette, with
    SGR-dim secondary text, terminal-detected card fills, or configurable outline borders.

- **[Keyboard Shortcuts & Controls](docs/shortcuts.md)**
  - Comprehensive keybinding reference organized by session flow, queueing,
    model controls, and editor navigation.

- **[Workflows](docs/workflows.md)**
  - Multi-agent workflows defined in XML and started with `/workflow <name>`.
  - Watch any running agent live with `Left`.

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

Tasks run through [mise](https://mise.jdx.dev), which also installs the pinned tools (nextest, cccc, jq). Run
`mise install` once, then:

```sh
mise run all          # fmt check, clippy, headless clippy, complexity, tests, site checks
V=1 mise run test     # every test and compile line, even where CI is set (agents, CI runners)
Q=1 mise run build    # hide cargo's compile lines in a terminal too
mise run clippy
mise run leak-check   # runs the tests with a private TMPDIR and fails if any test leaves files behind
mise run www-serve    # build the website (www/, an nsh app) and serve it at http://127.0.0.1:4173/rho/
mise tasks            # every task
```

---

## Acknowledgments

- Special thanks to [pi.dev](https://pi.dev) for inspiring aspects of rho's design and ergonomics.
