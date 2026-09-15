# Ratatui TUI and Dioxus Web UI Unification Spec

## Status

Draft

## Problem

`rho` currently maintains two disconnected presentation implementations:
1. A hand-rolled ANSI line-diffing, cursor-tracking, and clear-line terminal engine in Rust (`src/ui/interactive`, `src/repl/live`).
2. An imperative vanilla JavaScript and DOM-manipulation client for the remote Web Hub (`www/hub/js/`).

This separation causes significant friction:
- Every feature (modals, prompt editor, thinking accordions, tool status cards, session management, permissions) must be implemented twice.
- The custom terminal diffing engine is complex and prone to edge-case rendering glitches across terminal emulators.
- The Web Hub JS lacks type safety and requires manual DOM synchronization with WASM bindings.
- Maintaining bidirectional UI parity between terminal and browser requires double the maintenance effort.

## Users and stakeholders

- **CLI Developers**: Users running `rho` directly in their terminal who require responsive, flicker-free inline interaction, standard keyboard shortcuts, and intact terminal scrollback.
- **Remote Web Hub Users**: Users managing fleet nodes and chat sessions over peer-to-peer Iroh connections in the browser.
- **Maintainers**: Engineers developing new UI features, interactive prompts, and tool visualizers across all platforms.

## Goals

- Replace the custom ANSI terminal controller with standard Ratatui components using `Viewport::Inline` to maintain existing inline scrollback behavior.
- Adopt `ratatui-textarea` for prompt input buffer management, replacing ~620 lines of hand-rolled editor geometry, undo history, and cursor math while enabling first-class Vim mode emulation.
- Standardize diff generation and word diffs via the `similar` crate in `rho-ui-core`, deleting ~400 lines of custom LCS table indexing in `src/ui/render/diff.rs`.
- Replace hand-rolled fuzzy string scoring (`src/repl/interactive/fuzzy.rs`) with the existing workspace `fuzzy-matcher` crate.
- Replace manual container box-drawing (`src/ui/block/`) with Ratatui's native `Block` and `Borders` widgets.
- Eliminate external CLI prompting library `inquire` and progress bar crate `indicatif`, standardizing all interactive prompts and progress indicators on `rho-ui-core` state machines, Ratatui widgets (`Gauge`), and Dioxus components.
- Centralize `CompletionEngine` and `PromptHistory` into `rho-ui-core`, providing the Web Hub with instant feature parity for slash commands (`/`), file mentions (`@`), and `Up`/`Down` prompt history navigation.
- Unify RPC communication using strongly-typed `RpcCommand` and `RpcEvent` enums directly in Rust WASM, eliminating untyped JSON string maps in JavaScript.
- Centralize provider authentication definitions (`PROVIDER_DEFS`: OAuth vs API Key) in `rho-ui-core`, resolving feature drift across the CLI auth commands (`src/cli/auth/provider.rs`), the REPL modal (`login.rs`), and the Web Hub (`auth.js`).
- Unify relative time formatting (`format_relative_time`) and gutter line numbering in `rho-ui-core`, displaying human-friendly relative session timestamps across both TUI and Web Hub without manual string hacking.
- Centralize keyboard event normalization (handling platform quirks like macOS Shift+Tab / BackTab and Windows release filtering) in `rho-ui-core`.
- Unify transcript hydration and execution lifecycle (`Prompt`, `Steer`, `Abort`) in `use_session()`, eliminating duplicate turn scheduling logic in `www/hub/js/app.js`.
- Centralize footer metrics formatting (`FooterMetrics`: tokens, cost, tokens/sec, context window %), conversation tree navigation (`SessionTreeState`), startup welcome generation (`WelcomeDisplay`), and Iroh ticket parsing in `rho-ui-core`, eliminating ~200 lines of duplicate formatting and regex parsing JS in `www/hub/js/app.js`.
- Centralize the slash command registry (`SlashCommandDef`), skill exploration (`SkillModalState`), and prompt template expansion in `rho-ui-core`, eliminating `inquire` from `src/repl/commands/skill.rs` and giving the Web Hub an interactive Skill Explorer and custom slash command expansion.
- Persist editor preferences (`editor_mode: "vim" | "default"`) within `UiConfig` and browser `localStorage`.
- Delegate cursor positioning to Ratatui's native `frame.set_cursor_position` and `ratatui-textarea` styling, deleting manual `OutputTracker` cursor bookkeeping.
- Replace manual ASCII box-drawing and table border calculations (`src/ui/markdown/table/` - 269 lines) with Ratatui's native `Table` widget.
- Eliminate manual headless tool card formatting (`src/ui/render/card.rs` - 118 lines) in favor of the unified `ContentBlock::ToolCall` projection.
- Model `ModelItem` as a strongly-typed domain struct in `rho-ui-core`, eliminating the tab-delimited (`\t`) string-packing hacks in `src/repl/live/modal/model.rs`.
- Centralize MCP server state management (`McpModalState`) in `rho-ui-core`, bringing interactive MCP server toggle controls to the Web Hub.
- Centralize tool content kind and language detection in `rho-ui-core`, deleting redundant logic in `src/ui/render/preview.rs`.
- Unify session command execution across TUI modals, line mode, the Web Hub, and the RPC daemon via `SessionCommandExecutor`, shrinking `src/cli/rpc.rs` from 1,272 lines to ~150 lines and eliminating duplicate command handlers in `line_mode/dispatch.rs`.
- Delete the standalone terminal session picker engine (`src/ui/interactive/session_picker/` - 251 lines), standardizing CLI `--resume` selection on the shared Ratatui modal view.
- Centralize `RhoTicket` generation and ticket base64 parsing in `crates/rho-harness-core`, eliminating byte-for-byte copy-pasted extraction logic across `src/platform/remote/endpoint.rs`, `crates/rho-wasm/src/lib.rs`, and `www/hub/js/app.js`.
- Eliminate global static synchronization locks (`ACTIVE_APPROVALS`, `ACTIVE_STEERING`, `REMOTE_PROMPT_QUEUE` in `src/platform/remote/mod.rs`), encapsulating steering queues and approval responders directly inside `use_session` in `rho-ui-core`.
- Unify remote peer pairing state (`RemotePairingState`) across the TUI `/remote` dialog and Web Hub node pairing flow.
- Collapse the fragmented dual execution loops (`idle_loop` in `src/repl/live/idle/` vs. `turn_loop` in `src/repl/live/turn/` - ~2,500 lines) into a single unified reactive runloop in the terminal, managing keystrokes, steering, cancellation, and redraws uniformly regardless of turn execution state.
- Unify `UiEvent` and `RpcEvent` into a single canonical event schema in `rho_harness_core`, eliminating the dual-event hierarchy and translation adapters across `StructuredPresenter`, `BroadcastPresenter`, and the Web Hub.
- Centralize prompt queuing, follow-ups, and steering coordination (`PromptQueueCoordinator`) in `rho-ui-core`, giving the Web Hub full parity for queuing messages while turns are actively running.
- Centralize dynamic model discovery and capability descriptors (`ModelRegistry`: context sizes, reasoning badges, local/ollama detection) in `rho-ui-core`, sharing model metadata across the CLI modal, autocomplete, and the Web Hub.
- Delete redundant tool stream wrappers (`src/ui/stream.rs`), routing tool stream chunks directly via canonical `ToolChunk` events.
- Eliminate custom terminal input worker threads and pause/resume plumbing (`src/repl/input_reader/` - 320 lines), reading `crossterm::event::EventStream` directly inside the primary async `tokio::select!` loop.
- Unify bash escape expansions (`!cmd` local execution, `!!cmd` prompt context injection) and turn usage updates in `use_session()` in `rho-ui-core`, providing Web Hub users with remote node bash escapes.
- Eliminate over 500 lines of duplicate command handlers in `src/repl/live/message.rs` by routing through `SessionCommandExecutor`.
- Collapse fragmented presenter adapters (`BroadcastPresenter`, `RpcPresenter`, `TerminalRenderer`, and `ThinkingStreamTracker` - ~700 lines) into direct canonical event async broadcast channels feeding `rho-ui-core` state reducers, eliminating dual-mode (`if has_ui`) branching across every renderer call.
- Eliminate custom dual-slot transcript render caches (`src/ui/interactive/controller/cache.rs` - 146 lines and 272 lines of tests), relying on Ratatui's high-speed in-memory rendering loop without caching rendered ANSI strings.
- Consolidate duplicate tool card formatting (`src/ui/interactive/transcript/tool.rs` - 171 lines and `src/ui/render/card.rs` - 118 lines) into the unified `ContentBlock::ToolCall` projection.
- Eliminate custom event batching queues (`LiveBatch`, `PendingUiBatch` - 617 lines), relying on Ratatui's native in-memory double-buffering to prevent display tearing.
- Centralize settings configuration (`SettingsState`) in `rho-ui-core`, providing the Web Hub with an interactive Settings dialog and synchronizing preferences (thinking visibility, tool expansion, Vim mode).
- Unify global keyboard shortcuts (double-escape tree navigation, `Alt+T` to cycle thinking, `Alt+P`/`Alt+N` to cycle models) into `rho-ui-core` action reducers.
- Prune intermediate styling crate `anstyle` in favor of Ratatui's native `Style` and `Color` definitions.
- Support native 24-bit TrueColor RGB in syntax highlighting via Ratatui `Color::Rgb(r, g, b)`, deleting ~60 lines of custom color downsampling and grayscale quantization math in `src/ui/markdown/highlight.rs`.
- Eliminate redundant `crossterm::terminal::size()` ioctl syscalls across 7 different modules, relying on Ratatui's cached `frame.area()` updated on `Event::Resize`.
- Eliminate manual character-by-character word wrapping math (`src/ui/interactive/layout/text.rs` - 216 lines), delegating terminal text wrapping to Ratatui's native `Paragraph::wrap(Wrap { trim: true })` and web wrapping to native CSS.
- Eliminate custom ANSI markdown streaming compilers (`src/ui/markdown/renderer.rs` - 357 lines, `src/ui/markdown/stream.rs` - 366 lines, and `src/ui/markdown/elements.rs` - 101 lines), delegating markdown parsing directly to `pulldown-cmark` events constructing `ContentBlock` and `InlineSpan` without manual escape sequence tracking or word boundary regexes.
- Standardize markdown parsing on `pulldown-cmark`, deleting ~276 lines of manual regex line scanning and blank-line spacing state machines in `src/ui/markdown/line.rs` and `spacing.rs`.
- Prune 20+ redundant low-level text editor keybinding definitions from `src/ui/interactive/keybinding_loader/`, delegating standard Emacs/Readline editing actions directly to `ratatui-textarea`.
- Unify interactive tool execution and bash streaming directly through reactive tool state signals in `rho-ui-core`, eliminating custom channel polling loops in `src/repl/live/bash_runner/`.
- Provide browser-native session exporting (Markdown/HTML download) in the Web Hub by reusing `rho_harness_core::session::export` in WASM, and standardize artifact export rendering directly on `ContentBlock` to eliminate duplicate message loop iterators.
- Centralize `SessionSummary` display title and relative timestamp formatting in `rho-ui-core`, eliminating client-side string truncation fallbacks in JavaScript.
- Eliminate hardcoded string prefix checking for command arguments in `src/repl/interactive/completion/args.rs`, driving argument completions declaratively from `SlashCommandDef` argument types (`SlashArgumentType`) in `rho-ui-core`.
- Centralize ephemeral system feedback and transient toasts (`use_toast`) in `rho-ui-core`, eliminating custom 3-second expiration math in `src/ui/interactive/controller/system_message.rs` and enabling animated toast components in the Web Hub.
- Replace hardcoded provider model lists in `rho models` (`src/cli/commands.rs`) with dynamic model discovery from `ModelRegistry`.
- Standardize block notices and warning banners on `ContentBlock::Notice`, eliminating manual ASCII border padding and background formatting in `src/ui/render/notices.rs`.
- Unify multimodal image attachment handling (`ImageAttachment`) in `rho-ui-core`, standardizing terminal clipboard screenshot pasting (`arboard` / `[image ...]`) and Web Hub drag-and-drop image uploads into structured `RpcCommand::Prompt` payloads.
- Index both prompt templates (`.rho/prompts/*.md`) and skills (`.agents/skills/*/SKILL.md`) in `CompletionEngine`, expanding templates declaratively upon turn submission.
- Eliminate manual divider character concatenation and string repetition (`src/ui/interactive/layout/chrome.rs` - 159 lines), delegating top divider banners and thinking border colors to Ratatui's native `Block::borders(Borders::TOP)`.
- Strongly type tool argument presentation via `ToolInvocation` in `rho-ui-core`, eliminating fragile string indexing (`args.get("path")`, `args.get("format")`) across tool inspectors.
- Automatic credential and sensitive secret redaction via `SecretGuard` in `rho-ui-core` before text is projected to terminal cells or Web Hub components.
- Centralize window focus state tracking (`FocusGained` / `FocusLost` in TUI and browser window focus in Web Hub), dimming or brightening active chrome accents reactively.
- Replace ~960 lines of handwritten CSS in `www/hub/css/hub.css` with standard modern styling and accessible component primitives from Dioxus Components (`Card`, `Dialog`, `Accordion`, `Button`, `Input`, `Badge`).
- Replace the imperative JavaScript Web Hub frontend with a Dioxus-based WebAssembly application utilizing Dioxus Components.
- Introduce a shared Rust UI presentation crate (`crates/rho-ui-core`) powered by Dioxus reactivity (`dioxus-core` / `dioxus-signals`) that encapsulates:
  - Reactive signals (`Signal<T>`) for transcripts, prompt editor, modal stack, and active tool states.
  - Reusable custom hooks (`use_session`, `use_modal`, `use_permission_prompt`, `use_autocomplete`, `use_streaming_transcript`).
  - Shared RPC and event dispatching logic.
- Achieve strict bidirectional feature parity between native terminal and Web Hub interfaces with zero duplicated domain presentation code.

## Non-goals

- Altering the core AI execution loop, tool execution logic, or provider clients in `crates/rho-engine`.
- Changing the underlying Iroh RPC transport protocol or frame schema (`/rho/rpc/v1`).
- Switching the native terminal UI to a full-screen alternative screen buffer (the inline scrollback workflow must be preserved).
- Migrating static documentation or landing pages (`www/index.html`, `www/docs.html`) to Dioxus.

## Current behavior

- Terminal:
  - Hand-rolled cursor positioning and line diffing in `src/ui/interactive/controller/paint.rs` and `backend.rs`.
  - Manual escape code injection (`CSI_BEGIN_SYNC_UPDATE`, `CSI_END_SYNC_UPDATE`) and raw mode handling via `crossterm`.
  - In-house modal positioning and widget drawing across `src/ui/interactive/layout/`.
  - High test and maintenance surface for custom terminal bookkeeping (`OutputTracker`, line wrapping calculations).
- Web Hub:
  - Vanilla JavaScript in `www/hub/js/` (`app.js`, `client.js`, `session.js`, `auth.js`, `registry.js`).
  - Hand-crafted DOM element creation and innerHTML updates.
  - `crates/rho-wasm` exposes thin serialization helpers (`parse_ticket`, `encode_rpc_request`, `process_stream_content`, `IrohPeer`) called by JS.

## Desired behavior

- **Domain Presentation Core (`crates/rho-ui-core`)**:
  - Built on Dioxus reactive signals and hooks.
  - Houses the Semantic UI Block Intermediate Representation (`ContentBlock`, `InlineSpan`, `StyleToken`): markdown, code fences, diffs, tables, diagrams, and thinking blocks are parsed once into typed blocks.
  - Exposes fine-grained reactive state (`Signal<Vec<ContentBlock>>`, `Signal<ModalState>`, `Signal<PromptInput>`) and custom hooks.
  - Exposes deterministic state reducers for user input, streaming RPC events, tool execution states, and modal navigation.
- **Terminal UI (`src/ui/` & `src/repl/`)**:
  - Implemented with Ratatui using `ratatui::Viewport::Inline(height)`.
  - Driven by a headless Dioxus runtime loop that triggers Ratatui redraws whenever signals mutate or coroutines emit events.
  - Acts as a thin projection adapter: maps `ContentBlock` directly to Ratatui widgets (`Paragraph`, `Table`, `Line`) and stdout scrollback lines without re-parsing markdown or diffs.
  - Prompt editing powered by `ratatui-textarea`, supporting standard input mode or modal Vim mode (Normal, Insert, Visual, Replace, motions, operators) toggled via settings.
  - Prints finalized turn outputs into stdout scrollback once completed, erasing the inline viewport and restoring normal terminal flow.
- **Web Hub (`www/hub/` & `crates/rho-wasm`)**:
  - Full single-page application written in Rust using Dioxus and Dioxus Components.
  - Directly drives Iroh P2P connections and RPC sessions via `rho-wasm`.
  - Acts as a thin projection adapter: maps the identical `ContentBlock` stream into Dioxus Components (`Accordion`, code fences with copy buttons, HTML tables, SVG diagrams).
  - Deployed as a compiled WebAssembly binary + minimal HTML entry point (`index.html`).

## Architecture

```
+-------------------------------------------------------------+
|                      crates/rho-ui-core                     |
|           [Dioxus Reactive Core: Signals & Hooks]           |
|                                                             |
|  - Semantic UI Block IR: ContentBlock, InlineSpan, DiffHunk |
|  - Unified Parsers: Markdown, Diff, Table, Mermaid, Stream  |
|  - Reactive Signals: Signal<Transcript>, Signal<Modals>     |
|  - Hooks: use_session(), use_modal(), use_permission_prompt()|
+------------------------------+------------------------------+
                               |
            +------------------+------------------+
            | (Signal subscriber)                 | (Dioxus RSX & Signals)
            v                                     v
+-----------------------+             +-----------------------+
|     Native TUI        |             |       Web Hub         |
|  - ratatui            |             |  - dioxus             |
|  - ratatui-textarea   |             |  - dioxus-components  |
|    (Vim/Default mode) |             |  - wasm32-unknown     |
|  - Viewport::Inline   |             |  - Iroh WebRTC / P2P  |
|  - Pure IR Projection |             |  - Pure IR Projection |
|  - stdout scrollback  |             |                       |
+-----------------------+             +-----------------------+
```

## Code Layout and De-fragmentation

The current UI layer suffers from deep nesting (up to 6–7 directory levels, e.g. `src/ui/interactive/layout/modal/horizontal.rs`) and micro-fragmentation across dozens of tiny 20–50 line files. This reorganization collapses the UI footprint into shallow, cohesive packages targeting <= 3–4 levels:

### 1. `crates/rho-ui-core` (Shared Presentation Layer, <= 2 levels deep)
```
crates/rho-ui-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Crate root, re-exports
    ├── ir.rs               # ContentBlock, InlineSpan, DiffHunk, StyleToken
    ├── parser.rs           # Tokenizer for markdown, tables, diffs, <thinking>
    ├── state.rs            # Dioxus reactive signals & root state
    ├── session.rs          # use_session hook: turn lifecycle & streaming
    ├── modal.rs            # use_modal hook: selection, search, pagination
    ├── permission.rs       # use_permission_prompt: tool gates & prefill
    └── autocomplete.rs     # use_autocomplete hook: slash commands & paths
```

### 2. Native TUI in `src/ui/` (<= 3 levels deep)
Consolidates ~40 fragmented layout and state files into 4 cohesive modules:
```
src/ui/
├── mod.rs                  # TUI interface entry
├── terminal.rs             # Ratatui runner with Viewport::Inline & resize
├── view.rs                 # Pure projection: ContentBlock -> Ratatui widgets
├── editor.rs               # ratatui-textarea + Vim transition state machine
└── modal.rs                # Centered dialogs, permission prompt, autocomplete
```
*Deletions*: Deletes `src/ui/interactive/layout/` (13 files), `src/ui/interactive/state/editor/` (6 files), `src/ui/interactive/session_picker/` (2 files), `src/ui/interactive/controller/paint.rs`, `screen_sim.rs`, `src/repl/live/` (21 files), `src/repl/line_mode/` (7 files), `src/repl/input_reader/` (4 files), `src/repl/coordinator/` (4 files), `src/ui/stream.rs`, `src/repl/completer.rs`, and `src/repl/prompt.rs`.

### 3. Web Hub in `crates/rho-wasm` / `www/hub/` (<= 3 levels deep)
Replaces all 5 imperative JS files with cohesive Dioxus components:
```
crates/rho-wasm/
├── Cargo.toml
└── src/
    ├── lib.rs              # WASM entry & Iroh peer transport hook
    ├── app.rs              # Root Dioxus component (Fleet vs Workspace view)
    ├── fleet.rs            # Fleet node grid & pairing modal
    ├── workspace.rs        # Chat transcript: ContentBlock -> Dioxus Components
    └── modal.rs            # Auth, settings, and permission approval dialogs
```
*Deletions*: Deletes `www/hub/js/app.js`, `auth.js`, `client.js`, `registry.js`, and `session.js`.

## Requirements

### Architecture and Reactive Core
- **REQ-001**: A new workspace crate `crates/rho-ui-core` must be established, compiling cleanly on both native targets and `wasm32-unknown-unknown`.
- **REQ-002**: `rho-ui-core` must implement UI state management using Dioxus reactive signals (`Signal<T>`), memos, and custom hooks (`use_session`, `use_modal`, `use_permission_prompt`, `use_autocomplete`).
- **REQ-003**: `rho-ui-core` must define a Semantic UI Block Intermediate Representation (`ContentBlock`, `InlineSpan`, `DiffHunk`, `StyleToken`) that models headings, paragraphs, code fences, diffs, tables, diagrams, and thinking drawers independently of any renderer.
- **REQ-004**: All markdown parsing, diff tokenization, table structuring, and streaming tag extraction must execute exclusively within `rho-ui-core` via `pulldown-cmark`, producing typed `ContentBlock` and `InlineSpan` structures so that neither the TUI nor Web Hub performs bespoke string parsing, regex extraction, or ANSI escape tracking (`MarkdownRenderer` and `StreamWordWrapper` deleted).
- **REQ-005**: Diff generation for tool edits must be powered by the `similar` crate in `rho-ui-core`, eliminating custom LCS table calculations and character categorization.
- **REQ-006**: Fuzzy filtering in modals and autocomplete must standardize on `fuzzy-matcher` (`SkimMatcherV2`), deprecating hand-rolled scoring in `src/repl/interactive/fuzzy.rs`.
- **REQ-007**: Semantic styling tokens (`ThemeTokens`) must be declared in `rho-ui-core`, mapping to `ratatui::style::Style` in the TUI and CSS variables in the Web Hub.
- **REQ-008**: The autocomplete engine (`CompletionEngine`) and prompt history tracker (`PromptHistory`) must live in `rho-ui-core`, sharing completion candidates and draft navigation between TUI and Web Hub.
- **REQ-009**: The Web Hub and TUI must exchange typed messages exclusively via `rho_harness_core::rpc::protocol` (`RpcCommand`, `RpcEvent`), eliminating untyped JS string manipulation and custom serialization wrappers.
- **REQ-010**: Footer metrics calculation and formatting (token counts, context %, cost, tokens/sec, quota pills) must be implemented once in `rho-ui-core`, eliminating duplicate JavaScript formatting in `www/hub/js/app.js`.
- **REQ-011**: Session tree visualization and checkpoint branching (`/tree`, `/fork`) must be managed by a shared `SessionTreeState` in `rho-ui-core`, rendering in both TUI and Web Hub.
- **REQ-012**: Startup welcome cards, tool categorization (built-in, MCP, custom), and session notice formatting must be generated exclusively by `rho-ui-core`.
- **REQ-013**: Provider authentication definitions (`PROVIDER_DEFS`) must be centralized in `rho-ui-core`, serving as the single source of truth across CLI commands (`rho auth login`), the REPL `/login` modal, and the Web Hub auth dialog.
- **REQ-014**: Code block presentation in `ContentBlock::CodeBlock` must natively support line numbering and gutter width offsets, eliminating manual tabbed line parsing (`parse_read_line`) in renderers.
- **REQ-015**: Session relative timestamps, display title formatting, and summary diagnostics must be implemented in `rho-ui-core`, ensuring consistent time representations across TUI and Web Hub session lists without client-side fallback hacks.
- **REQ-016**: Session message hydration and artifact exporting (`export.rs`) must standardize on `ContentBlock` in `rho-ui-core`, eliminating duplicate private block enums and redundant message iteration loops.
- **REQ-017**: Session turn execution lifecycle transitions (`Prompt` when idle, `Steer` when running, `Abort` on cancel) must be managed exclusively by `use_session` in `rho-ui-core`.
- **REQ-018**: Iroh pairing ticket extraction and decoding must be standardized in `rho_harness_core::rpc::ticket`, eliminating duplicate copy-pasted extraction logic across `src/platform/remote/endpoint.rs`, `crates/rho-wasm/src/lib.rs`, and `www/hub/js/app.js`.
- **REQ-019**: Active approvals, steering queues, and prompt channels must be encapsulated directly within `use_session` in `rho-ui-core`, eliminating unsafe global static singletons (`ACTIVE_APPROVALS`, `ACTIVE_STEERING`, `REMOTE_PROMPT_QUEUE` in `src/platform/remote/mod.rs`).
- **REQ-020**: Dynamic model discovery and capability descriptors (`ModelRegistry`: context limits, reasoning effort, local model detection) must be centralized in `rho-ui-core`, replacing hardcoded model strings in `rho models` (`src/cli/commands.rs`) and sharing metadata across autocomplete, the CLI `/model` modal, and the Web Hub model selector.
- **REQ-021**: Ephemeral feedback and system notices must be managed by `use_toast` in `rho-ui-core` with automatic expiration, rendering as divider status text in the TUI and animated toast components in the Web Hub (`src/ui/interactive/controller/system_message.rs` deleted).
- **REQ-022**: Prompt queuing, follow-up messages, and steering transitions must be managed by `PromptQueueCoordinator` in `rho-ui-core`, enabling the Web Hub to queue prompts while turns are executing.
- **REQ-023**: The slash command registry (`SlashCommandDef`) must be declared once in `rho-ui-core` with typed argument definitions (`SlashArgumentType`), automatically generating command lists, argument autocomplete candidates, `/help` reference strings, and Web Hub command palettes without hardcoded string prefix matching.
- **REQ-024**: MCP server configuration and toggle states must be managed by `McpModalState` in `rho-ui-core`, enabling the Web Hub to inspect and toggle MCP servers.
- **REQ-025**: Installed skills and prompt templates must be inspectable and expandable via `SkillModalState` in `rho-ui-core`, replacing `inquire` in `src/repl/commands/skill.rs` and providing the Web Hub with interactive skill browsing.
- **REQ-026**: User settings configuration (`SettingsState`: thinking output toggle, tool expansion toggle, vim mode, version banner) must live in `rho-ui-core`, rendering in both the TUI `/settings` modal and a Dioxus Web Hub Settings dialog.
- **REQ-027**: Global shortcuts (double-escape tree navigation, `Alt+T` thinking cycling, `Alt+P`/`Alt+N` model cycling) must be handled by shared action reducers in `rho-ui-core`.
- **REQ-028**: External dependencies (`inquire`, `indicatif`, `reedline`, `anstyle`) and custom event batch queues (`PendingUiBatch` / `LiveBatch` - 617 lines) must be completely removed, relying on Ratatui's native offscreen double-buffering.
- **REQ-029**: Session command execution (`SessionCommandExecutor`: fork, clone, resume, compact, model/thinking switches) must be implemented once in `rho-ui-core`, unifying execution paths across TUI modals, line-mode prompts, remote RPC commands, and the `rho rpc` daemon.
- **REQ-030**: A single canonical event schema (merging `UiEvent` and `RpcEvent`) must be used universally across the execution engine, headless JSON streaming, test sinks, and network transports, eliminating parallel event enum hierarchies.
- **REQ-031**: Terminal user input must be read directly via `crossterm::event::EventStream` within the async runloop, completely deleting the threaded reader and pause/drain mechanisms in `src/repl/input_reader/`.
- **REQ-032**: Bash escape prefixes (`!cmd` and `!!cmd`) must be resolved by `use_session` in `rho-ui-core`, ensuring consistent shell execution and context injection across TUI and Web Hub.
- **REQ-033**: Direct canonical event broadcast channels must replace the multi-tiered presenter abstraction (`BroadcastPresenter`, `RpcPresenter`, `TerminalRenderer`), feeding typed events directly to `rho-ui-core` state reducers without intermediate wrappers.
- **REQ-034**: The startup session picker (`rho --resume`) must reuse the shared Ratatui modal renderer, completely deleting the standalone terminal session picker engine in `src/ui/interactive/session_picker/`.
- **REQ-035**: The modal and user selection state machine in `rho-ui-core` must support unified navigation semantics (up/down, horizontal navigation, enter to select, escape to dismiss, search filtering, digit jump keys, and input mode transitions) shared across TUI and Web.
- **REQ-036**: State mutations and streaming events must update signals directly, ensuring both TUI and Web run the identical state transition logic without translation layers.

### Ratatui Terminal Interface
- **REQ-037**: The terminal interactive runner must use Ratatui with `Viewport::Inline` to render the bottom interactive area (prompt editor, autocomplete menu, active tool progress, and modals).
- **REQ-038**: The terminal interactive runner must use a single unified reactive runloop, collapsing the legacy separate `idle_loop` and `turn_loop` (~2,500 lines across `src/repl/live/idle/` and `turn/`) into a unified event-driven cycle that handles input, steering, and redraws consistently.
- **REQ-039**: The prompt input buffer must be managed by `ratatui-textarea`, deprecating bespoke cursor, geometry, and kill-ring logic in `src/ui/interactive/state/editor/`, and delegating all standard Emacs/Readline keys natively.
- **REQ-040**: The prompt editor must support a configurable Vim mode (Normal, Insert, Visual, Replace, motions `h`/`j`/`k`/`l`/`w`/`b`/`$`/`^`, operators `d`/`y`/`c`, undo/redo) following the `ratatui-textarea` transition state machine.
- **REQ-041**: Box borders, containers, and cards in the TUI must standardize on Ratatui's native `Block` and `Borders`, deleting `src/ui/block/`.
- **REQ-042**: Text word-wrapping and visual line calculations in the TUI must be handled natively by Ratatui (`Paragraph::wrap`), deleting bespoke wrapping arithmetic in `src/ui/interactive/layout/text.rs`.
- **REQ-043**: Tables in the terminal must be rendered using Ratatui's native `Table` widget, deleting manual ASCII border formatting in `src/ui/markdown/table/`.
- **REQ-044**: Markdown parsing must standardize on `pulldown-cmark` in `rho-ui-core`, deleting regex line scanning and blank spacing tracking in `src/ui/markdown/line.rs` and `spacing.rs`.
- **REQ-045**: Interactive bash and tool execution streaming must flow into `Signal<ActiveToolState>` in `rho-ui-core`, eliminating custom channel draining loops and timer tickers in `src/repl/live/bash_runner/`.
- **REQ-046**: The inline viewport height and modal overlays must dynamically clamp to available rows without overflowing terminal boundaries, delegating bounds calculations to Ratatui layout constraints and eliminating `src/ui/interactive/layout/budget.rs`.
- **REQ-047**: When an execution turn finishes, the completed turn content must be written directly to terminal scrollback wrapped in OSC 133 semantic prompt marks (`OSC133_ZONE_START` / `OSC133_ZONE_END`) to maintain jump-to-previous-turn terminal navigation in modern terminals (Ghostty, iTerm2, WezTerm).
- **REQ-048**: The terminal interactive runner must support mouse interaction (clicking modal rows, accordion toggles, and approval buttons) via crossterm mouse capture.
- **REQ-049**: Terminal keybinding semantics must remain identical: `Escape` cancels/dismisses, `Ctrl+C` clears input drafts, `Ctrl+D` exits when prompt is empty, and standard arrow/vi keys navigate modals.
- **REQ-050**: Custom ANSI painting and cursor diffing logic in `src/ui/interactive/controller/paint.rs` and `ansi.rs` must be completely removed.

### Dioxus Web Hub Interface
- **REQ-051**: All JavaScript application logic in `www/hub/js/` (`client.js`, `app.js`, `session.js`, `auth.js`, `registry.js`) must be replaced by a Dioxus application compiled to WebAssembly.
- **REQ-052**: The Dioxus application must render the Fleet view, Active Node workspace, Session sidebar, Chat transcript, and Modals using Dioxus Components, deprecating manual custom CSS in `www/hub/css/hub.css`.
- **REQ-053**: OAuth and credential input callbacks must standardize on `RpcAuthBridge` in `rho_harness_core`, eliminating blocking `inquire` input tasks in `src/cli/auth/callbacks.rs`.
- **REQ-054**: The Web Hub must render inline image previews with modal zoom and support 1-click in-browser session exporting (Markdown and HTML file download) reusing `rho_harness_core::session::export`.
- **REQ-055**: Peer-to-peer connectivity via Iroh must operate directly within Rust WebAssembly in `crates/rho-wasm`, deserializing canonical events directly without crossing `JsValue` boundaries or delegating to JavaScript callbacks.
- **REQ-056**: Local storage persistence (node tickets, saved sessions, sidebar toggle states) must be managed via web-sys wrappers within the Dioxus application.
- **REQ-057**: The Web Hub build pipeline must integrate into `make wasm`, producing a production-ready WASM bundle and asset structure.

### UI Parity and Interaction
- **REQ-058**: All interactive modals (thinking selector, model picker, login provider, MCP servers, session list) and user selection screens must present the same options, indicators (active checkmarks), and search filtering in both interfaces.
- **REQ-059**: Tool permission and approval prompts (`Allow once`, `Allow always`, `Deny with reason`, `Edit command`) must be driven by `use_permission_prompt`, supporting inline parameter editing via `ratatui-textarea` in the TUI and Dioxus form components in the Web Hub.
- **REQ-060**: Thinking blocks must stream in real time with duration counters and support expandable/collapsible accordion display on both platforms.
- **REQ-061**: Tool execution cards must visually represent status states (running, success, error) and structured output (including syntax-highlighted diffs) consistently across both TUI and Web Hub.
- **REQ-062**: Markdown rendering must preserve high-fidelity presentation: code block syntax highlighting, table borders, and Mermaid diagram rendering (ASCII diagram rendering via `merman` in TUI, interactive SVG in Web Hub).
- **REQ-063**: Streaming token chunks must be parsed by a unified `StreamChunkParser` in `rho-ui-core`, emitting typed `StreamEvent`s for content deltas, `<thinking>` blocks, and tool executions.
- **REQ-064**: Bracketed paste handling must preserve collapsed paste markers (`[paste #1 +50 lines]`) when pasting large multiline blocks into `ratatui-textarea` and web input, expanding automatically upon turn submission.
- **REQ-065**: Multimodal image attachments must be represented as `ImageAttachment` in `rho-ui-core`, standardizing terminal clipboard pasting (`arboard`) and browser drag-and-drop uploads into structured `RpcCommand::Prompt` payloads.
- **REQ-066**: Clipboard image pasting (via `arboard` in TUI and Web Clipboard API in Web Hub) must store image assets and insert reference tokens (`[image /tmp/...]`) seamlessly.
- **REQ-067**: Terminal job suspension (`Ctrl+Z` / `SIGTSTP`) and external subshell execution must cleanly suspend Ratatui raw mode, show the cursor, and restore the inline viewport upon resumption.
- **REQ-068**: Non-TTY and piped execution environments (`!is_terminal()`) must bypass Ratatui entirely, preserving line-mode and batch CLI behavior.
- **REQ-069**: TUI unit and integration tests must standardize on Ratatui's `TestBackend`, replacing custom mock terminal simulators (`HistoryTerminal`, `RedrawCountingTerminal`, `screen_sim.rs`).
- **REQ-070**: Session conversation exporting (`export.rs`) must project `ContentBlock` directly to Markdown and HTML, eliminating private block enums and duplicate message iteration loops.
- **REQ-071**: Custom prompt templates and installed skills must be indexed by `CompletionEngine` in `rho-ui-core` and expanded via `PromptTemplate::expand` prior to turn execution.
- **REQ-072**: Custom transcript render caches (`CachedItemRender`, `TranscriptRenderCache`) must be completely deleted, rendering frames in-memory without string caching.
- **REQ-073**: Tool card transcript formatting must be consolidated into `ContentBlock::ToolCall`, deleting duplicated formatting logic across `src/ui/interactive/transcript/tool.rs` and `src/ui/render/card.rs`.
- **REQ-074**: Code syntax highlighting must leverage native 24-bit TrueColor `Color::Rgb` in Ratatui, completely deleting custom RGB-to-ANSI16 color quantization logic in `src/ui/markdown/highlight.rs`.
- **REQ-075**: Terminal dimensions must be read exclusively from Ratatui's cached `frame.area()`, eliminating ad-hoc `crossterm::terminal::size()` ioctl syscalls on render hot paths.
- **REQ-076**: Top dividers, version banners, and activity indicators in the TUI must be rendered using Ratatui's native `Block::borders(Borders::TOP)` with title alignment, completely deleting manual `"─".repeat(...)` string formatting in `src/ui/interactive/layout/chrome.rs`.
- **REQ-077**: Tool argument inspection and file previews must deserialize into typed `ToolInvocation` variants (`ReadArgs`, `EditArgs`, `WriteArgs`, `WebFetchArgs`, `BashArgs`), eliminating untyped `serde_json::Value` string indexing in presentation layers.
- **REQ-078**: Transcript presentation in both TUI and Web Hub must run through `SecretGuard::redact` in `rho-ui-core` before rendering, ensuring zero accidental leakage of auth keys or session secrets.
- **REQ-079**: Terminal and browser focus state changes (`FocusGained` / `FocusLost`) must update a shared `Signal<WindowFocus>` in `rho-ui-core` to reactively dim or highlight active input chrome.

## Invariants and security boundaries

- **Zero Secret Exposure**: Provider API keys, credentials, and authentication tokens must never be logged, persisted in plaintext web storage, or transmitted across unencrypted channels.
- **Decoupled Business Logic**: `crates/rho-ui-core` must depend only on harness/engine types, never on transport (crossterm, websocket, or iroh) or rendering engines (ratatui, dioxus).
- **Terminal Scrollback Integrity**: Inline viewport rendering in the TUI must never corrupt terminal scroll history or leave phantom cursor lines upon exit or terminal resize.
- **Iroh P2P Transport Safety**: The Web Hub must communicate with rho nodes solely over end-to-end encrypted Iroh connections using ALPN `/rho/rpc/v1`.

## Definition of done

1. `crates/rho-ui-core` is created and shared by `rho` CLI and `crates/rho-wasm`.
2. Hand-written ANSI diffing/painting code in `src/ui/interactive` is replaced with Ratatui `Viewport::Inline`.
3. `www/hub/js/` is eliminated; the Web Hub is built entirely in Rust via Dioxus and Dioxus Components.
4. All existing TUI interactive tests pass or are updated to test against the Ratatui backend and `rho-ui-core`.
5. Running `make wasm` builds the Dioxus Web Hub and tests pass.
6. `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` succeed with zero errors.

## Risks and mitigations

- **Risk**: Ratatui inline viewport handling during concurrent terminal writes or background log streaming can cause terminal jitter.
  - **Mitigation**: Route all background logging and RPC events through synchronized state update channels that trigger controlled Ratatui frame renders.
- **Risk**: Dioxus WASM binary size could impact initial Web Hub page load speed.
  - **Mitigation**: Enable `lto = "fat"`, `codegen-units = 1`, `opt-level = "z"` / `"s"`, and run `wasm-opt` during `make wasm`.
- **Risk**: Incompatibilities between desktop Ratatui layout abstractions and web flexbox/grid layout models.
  - **Mitigation**: Keep layout calculations isolated to their respective rendering layers; `rho-ui-core` provides structured data models and action reducers, not pixel/character layouts.

## Out of scope

- Converting static landing and documentation pages (`www/index.html`, `www/docs.html`, `www/packages.html`) to Dioxus.
- Developing a native desktop GUI (e.g., Dioxus desktop with wry/tao).
- Rewriting the CLI non-interactive mode (`--print`, batch script execution).
