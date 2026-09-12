# Architecture

rho is a minimal, terminal-based agentic coding CLI. This document maps the
crate boundaries, the three execution loops, and the integration seams so a
new contributor can orient in minutes.

## Crate Boundaries

```
┌──────────────────────────────────────────────────────────────────┐
│ rho (binary + lib)                                               │
│  ├─ cli/       arg dispatch, auth, update, RPC server            │
│  ├─ repl/      REPL frontends (live TUI, line mode, headless)    │
│  ├─ ui/        markdown, transcript, interactive controller      │
│  └─ platform/  clipboard, terminal suspend                       │
├──────────────────────────────────────────────────────────────────┤
│ rho-engine         turn orchestration, providers, tools, MCP     │
├──────────────────────────────────────────────────────────────────┤
│ rho-harness-core   deterministic host domain (no UI/LLM wiring)  │
└──────────────────────────────────────────────────────────────────┘
```

Dependency direction is strictly downward: `rho → rho-engine → rho-harness-core`.

### `rho-harness-core` (host domain)

Deterministic domain code, independent of any framework types:

- `config/` — layered config (defaults → file → env → CLI) and clap CLI types.
- `session/` — durable JSONL session store: canonical history tree, branching,
  checkpointing, compaction bookkeeping, secret redaction, export.
- `workspace/` — path containment and mutation guards.
- `tokens/` — tiktoken-backed token estimation and cut-point selection.
- `presentation/` — UI-agnostic display contracts (`Presenter` trait, tool
  lines, activity tokens, structured NDJSON output).
- `provider/` — provider identity and capability enums.
- `prompts/`, `skills/`, `queue/`, `rpc/`, `net/` — prompt templates, skill
  discovery, message queueing, RPC transport, URL safety.

### `rho-engine` (agent runtime)

Everything needed to run model turns and tools:

- `engine/` — `AgentEngine` and the turn loop (below).
- `provider/` — model handle construction per provider (Claude, ChatGPT,
  Gemini/Antigravity, Ollama, OpenRouter, custom endpoints) and catalog
  discovery/presets.
- `claude/`, `antigravity/`, `chatgpt/`, `ollama/` — provider-specific wire
  formats, SSE streaming, quota parsing, and OAuth flows (`auth/`).
- `tools/` — built-in tools (bash, read, edit, write, fd, rg, web_fetch,
  web_search) plus shared plumbing: output truncation, atomic writes, HTTP
  client singleton, rate limiting.
- `permission/` — bash tokenizer, policy evaluation, interactive prompts.
- `mcp/` — MCP client/process/transport and tool gateway.
- `hook/` — lightweight one-shot process hooks (`.rho/hooks/`) for turn and tool lifecycle interception.

### `rho` (CLI shell)

- `cli/` — subcommand routing (`run`, `auth`, `mcp`, `update`, `rpc`), process
  cleanup guards, session resume plumbing.
- `repl/` — two frontends sharing one turn pipeline: `live/` (raw-mode TUI
  with modals, streaming transcript, autocomplete) and `line_mode/`
  (readline-style fallback). `coordinator/` decouples UI event pumping from
  turn execution.
- `ui/` — markdown rendering (streaming, tables, syntax highlighting), the
  interactive controller (transcript cache, redraw batching, keymaps), and
  terminal paint primitives.

## The Three Execution Loops

### 1. TUI Event Loop (`src/repl/live/`, `src/ui/interactive/`)

```
input ──▶ keymap ──▶ controller state ──▶ layout ──▶ paint
   ▲                                            │
   └──────────── events (batched) ◀─────────────┘
```

- `events/` funnels terminal input and engine output through a batched
  channel; a single pump drains it per frame.
- `controller/` owns transcript caching (standard/alternate render slots),
  in-place tool card updates, and synchronized-update (CSI 2026) redraws.
- `layout/` computes the line budget (editor, autocomplete, chrome, footer)
  from terminal size; modals reuse the same budget with caps.
- Modals (`repl/live/modal/`) are in-TUI state machines — raw mode is never
  suspended for prompts.

### 2. Agent Turn Loop (`rho-engine/src/engine/`)

```
TurnRequest
   │ prepare/ ─▶ context assembly (AGENTS.md, skills, transclusion)
   ▼
provider stream (claude/antigravity/chatgpt/ollama)
   │ stream.rs ─▶ DisplayEvent → sink ─▶ UI
   ▼
tool calls ── permission gate ──▶ execute ──▶ results appended
   │                                             │
   ▼                                             ▼
compactor/ (auto-compact on context overflow)  session store (JSONL)
   ▼
TurnOutput { final_text, usage, tool_calls }
```

- `runner/turn/` drives one turn: prepare → stream → tool dispatch →
  completion, with steering (`tool_hook/steering`), queued messages, and
  runtime model switching (`tool_hook/model_switch`).
- `runner/sink/` adapts engine events into `Presenter` calls with credential
  redaction.
- `tracking/` accumulates usage, tokens/sec, and quota snapshots.
- `compactor/` summarizes history on overflow or threshold, projecting
  compaction nodes into the session tree; durable history is never truncated.
- Session persistence (`rho-harness-core/src/session/`) appends every
  turn/tool result as JSONL with an append-only audit trail.

### 3. Tool Execution & Permission Gate

```
tool call ─▶ PermissionHook (engine/permission/)
                ├─ policy eval (project + global permission.toml)
                ├─ baseline allowlist (read-only, path checks)
                ├─ bash analysis (lexer ─▶ paths, redirection, suspicion)
                └─ interactive prompt (TUI modal / headless fail-closed)
                      │ allow / deny / edit / always-allow
                      ▼
              ToolRegistry dispatch (builtin, MCP)
                      ▼
              ToolResult (text + images) ─▶ session + UI
```

- `permission/bash/` tokenizes commands (quote-aware) and extracts path
  arguments so rules can target paths rather than raw strings.
- `Always allow` persists rules to the project permission surface and hot-
  reloads the in-memory policy.
- Headless/non-interactive runs (e.g. `--prompt` or `--mode json`) fail closed: any required prompt denies the call.

## Providers and MCP

- Each provider speaks its own wire protocol; `provider/builders.rs` selects
  the right client (OAuth token refresh, header quirks, streaming dialects).
  Model catalogs merge discovered models with curated presets; quota parsers
  are provider-specific (`antigravity/quota`, `chatgpt`, `ollama`).
- MCP servers spawn as child processes with JSON-RPC stdio transport
  (`mcp/process.rs`, `mcp/transport.rs`); tools surface through `McpGateway`
  and are reaped on engine rebuild. Configured via `~/.agents/mcp.json` or `.mcp.json`.
- Lifecycle hooks run as one-shot child processes (`hook/`) triggered by
  executables in `.rho/hooks/` (e.g. `on_tool_call`, `on_tool_result`),
  communicating via simple JSON on stdin/stdout without daemon overhead.

## Testing Layout

- Unit tests live in `#[cfg(test)] mod tests` blocks or sibling `tests.rs`
  files next to the code they cover.
- Integration tests live in the top-level `tests/` directory (headless runs,
  permission flows, hook execution, MCP).
- `engine/eval/` provides a deterministic mock harness (`MockCompletionModel`
  + scripted turns) used by both unit and eval tests; live-provider tests are
  opt-in and skipped without credentials.

## Conventions

- No Clippy suppressions; `clippy.toml` thresholds are the contract.
- HTTP clients are singletons with `.no_proxy()` (macOS IPC safety).
- File size targets are cohesive ~300–400 lines; avoid micro-fragmentation.
- `.specs/<slug>/spec.md` + `plan.md` capture feature specs before work.