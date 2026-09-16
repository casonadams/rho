# Ratatui TUI and Dioxus Web UI Unification Spec

## Status

In Progress (Phase 1 & Phase 2 Complete, Phase 3 Path A Underway)

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
- Support visual selection mode (`v`, `V`) and mouse selection in the terminal editor via `ratatui-textarea`, enabling direct yanking (`y`) and pasting (`p`) to and from the system clipboard with OSC 52 escape fallback for remote SSH sessions.
- Render interactive clickable hyperlinks for search results and fetched URLs in the Dioxus Web Hub transcript.
- Render context compaction milestones (`CompactionComplete`) as visual savings badges in both TUI scrollback and the Web Hub transcript.
- Provide accessible modal dialogs with automatic ARIA focus trapping (`focus-trap`) and keyboard navigation in the Dioxus Web Hub.
- Standardize tool output truncation presentation, allowing users to toggle expanded outputs (`Ctrl+O`) in the TUI or download full tool logs from `full_output_path` in the Web Hub.
- Validate and downsample large image attachments via client-side Rust WebAssembly (`fit_dimensions`) directly in the Web Hub, optimizing upload bandwidth prior to P2P network transmission.
- Centralize session deletion and pruning capabilities in `SessionStore`, allowing users to delete sessions from both the TUI `/session` modal and Web Hub sidebar.
- Centralize numeric token count and byte size formatting (`format_tokens`, `format_size`) in `rho-ui-core`, eliminating duplicate JavaScript numeric formatters in `www/hub/js/app.js`.
- Pre-tokenize code syntax on the host engine into structured `HighlightedLine` spans within `ContentBlock::CodeBlock`, preventing multi-megabyte `syntect` grammar sets from bloating the Web Hub WASM binary.
- Emit a terminal bell (`\x07`) notification on turn completion when window focus is lost, alerting users when background executions finish.
- Map container and card border preferences (`UiConfig::block_style`) directly to native Ratatui `BorderType` (Rounded, Plain, Double).
- Route background engine and compaction warnings through canonical `RpcEvent::Notice` channels rather than direct `eprintln!` calls, preventing terminal row desynchronization during inline viewport execution.
- Consolidate atomic file writes across binary self-update (`write_binary_atomically`) and tool modifications (`atomic_write`) in `rho_harness_core::fs`.
- Coalesce high-speed key-repeat movement events in the terminal event loop, mutating the in-memory editor buffer immediately and rendering at synchronized frame intervals to guarantee stutter-free cursor navigation.
- Standardize mouse wheel scroll velocity (3 lines per scroll notch) across modals, code previews, and transcript history in both interfaces.
- Automatically enforce repository modal standards (Title Case, fixed-width columns, active checkmarks `"  ✓"`, `Up`/`Down`/`k`/`j`/`Tab`/`Shift+Tab`, digit jump keys `1..=9`, search filtering) across all interactive selectors via `use_modal()` in `rho-ui-core`.
- Deduplicate YAML frontmatter parsing across skills and prompt templates in `rho_harness_core`.
- Standardize theme structures on `ratatui::style::Style` and `Color::Rgb` natively in `rho-ui-core`, eliminating `anstyle` completely from workspace dependencies and feature configurations.
- Encapsulate Ratatui behind clean trait abstractions (`TerminalSurface`, `TerminalComponent`, `PromptEditor`, `ModalView`), isolating application logic from raw Ratatui/textarea struct layouts and facilitating headless testability.
- Stream binary downloads during self-update (`download_asset`) to drive a visual Ratatui `Gauge` progress bar in the TUI and a progress bar component in the Web Hub.
- Enforce terminal restoration guards handling `SIGTERM`, `SIGHUP`, and panics to guarantee raw mode is always exited and cursor visibility restored.
- Support interactive node pairing (`/pair`) with visual QR codes rendered via Ratatui Unicode blocks in the TUI and SVG components in the Web Hub.
- Render URLs and web search citations as clickable OSC 8 hyperlinks in Ratatui Spans, providing parity with browser `<a>` anchors.
- Unify MCP server test diagnostics into structured `McpTestReport` in `rho_engine::mcp`, reused identically across CLI `rho mcp test`, the TUI `/mcp` modal, and the Web Hub MCP manager dashboard.
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
├── mod.rs                  # TUI interface entry & traits (TerminalSurface, TerminalComponent, PromptEditor, ModalView)
├── terminal.rs             # Ratatui runner with Viewport::Inline, resize, signals & panic guards
├── view.rs                 # Pure projection: ContentBlock -> Ratatui widgets via TerminalComponent
├── editor.rs               # TextAreaEditor implementing PromptEditor + Vim transition state machine
└── modal.rs                # Centered dialogs, permission prompt & autocomplete implementing ModalView
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
- **REQ-028**: External dependencies (`inquire`, `indicatif`, `reedline`, `anstyle`) and custom event batch queues (`PendingUiBatch` / `LiveBatch` - 397 lines across `src/ui/interactive/events/batch.rs` and `src/repl/live/batch.rs`) must be completely removed, relying on Ratatui's native offscreen double-buffering. **Audit note**: `reedline` is used in 4 files (`src/repl/line_mode/`, `completer.rs`, `prompt.rs`, `interactive/history.rs`). `inquire` has 10 call sites across `src/cli/auth/` and `src/repl/commands/`.
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
- **REQ-078**: Truncated tool outputs must present interactive expansion toggles (`Ctrl+O` in TUI) and allow full output log inspection/download via `full_output_path` in both interfaces.
- **REQ-079**: Transcript presentation in both TUI and Web Hub must run through `SecretGuard::redact` in `rho-ui-core` before rendering, ensuring zero accidental leakage of auth keys or session secrets.
- **REQ-080**: Terminal and browser focus state changes (`FocusGained` / `FocusLost`) must update a shared `Signal<WindowFocus>` in `rho-ui-core` to reactively dim or highlight active input chrome.
- **REQ-081**: Visual selection mode (`v`, `V`) and mouse selection in `ratatui-textarea` must support yanking and deleting directly to/from the system clipboard via `arboard`, with OSC 52 fallback for remote SSH terminal sessions.
- **REQ-082**: Modal dialogs in the Dioxus Web Hub must enforce accessible ARIA attributes (`role="dialog"`, `aria-modal="true"`) and automatic keyboard focus trapping.
- **REQ-083**: Tool outputs containing URLs (web search results, fetched links) must project as clickable hyperlinks in the Web Hub transcript.
- **REQ-084**: Completed context compaction events (`CompactionComplete`) must render as visual compaction milestones in both TUI scrollback and the Web Hub transcript.
- **REQ-085**: Image attachments in the Web Hub must be validated via client-side magic-byte sniffing (`detect_supported_image_mime`) and downsampled in Rust WebAssembly (`fit_dimensions`) before transmission to optimize P2P network bandwidth.
- **REQ-086**: Session deletion and pruning must be supported in `SessionStore`, allowing users to delete inactive sessions from both the TUI session modal and the Web Hub sidebar.
- **REQ-087**: Numeric formatting for token quantities (`format_tokens`) and byte memory sizes (`format_size`) must have a single canonical definition. **Audit note (2026-09-15, updated)**: the original 10k-vs-100k divergence between `src/ui/interactive/footer/text.rs` and `crates/rho-engine/src/engine/metrics/types.rs` is already resolved — both `footer/mod.rs` and `engine/metrics/types.rs` re-export `rho_harness_core::tokens::format_tokens`. The remaining duplication is `format_tokens`/`format_size` each defined in two places (`rho-harness-core::tokens` and `rho-ui-core::state`; `format_size` also in `rho-engine::tools::truncate`). Consolidate to one body and re-export (see REQ-105 / Slice 5).
- **REQ-088**: Code syntax highlighting must be pre-tokenized on the host into `HighlightedLine` spans inside `ContentBlock::CodeBlock`, keeping the Web Hub WASM bundle ultra-lean (<1.5MB gzipped) without compiling syntect language tables into browser WebAssembly.
- **REQ-089**: The terminal interactive runner must support an optional terminal bell (`\x07`) notification on turn completion when the terminal window is unfocused (`Signal<WindowFocus>` is false).
- **REQ-090**: Background engine and compaction warnings must be routed through canonical `RpcEvent::Notice` events rather than direct `eprintln!` calls, preventing terminal row desynchronization during inline viewport execution.
- **REQ-091**: All interactive selectors in both TUI and Web Hub must strictly enforce the repository `/thinking` modal standards (Title Case headers, empty subtitles, fixed-width option columns, active indicators `"  ✓"`, digit jump keys `1..=9`, and fuzzy filtering) via `use_modal` in `rho-ui-core`.
- **REQ-092**: Terminal key-repeat movement events must coalesce in the input channel, updating editor memory coordinates immediately and synchronizing frame redraws to eliminate cursor stutter at high repeat rates.
- **REQ-093**: Mouse wheel scrolling across modals, diff views, and transcript history must apply standardized velocity scaling (3 lines per notch) in both TUI and Web Hub.
- **REQ-094**: MCP server connectivity testing must yield a structured `McpTestReport` from `rho_engine::mcp`, displaying initialization duration, server capabilities, and discovered tool counts identically in CLI `rho mcp test`, TUI `/mcp` modal, and Web Hub.
- **REQ-095**: Theme definition and color palette detection must operate natively via `ratatui::style::Style` and `ratatui::style::Color`, eliminating all usage of `anstyle` and removing `anstyle` feature dependencies from `terminal-colorsaurus`.
- **REQ-096**: Interactive remote pairing (`/pair`) must display the endpoint ticket and visual QR code using Ratatui Unicode block elements in the TUI and SVG components in the Web Hub.
- **REQ-097**: URLs in markdown text, search results, and fetched pages must render as OSC 8 hyperlinks in Ratatui terminal spans where supported, matching clickable web hyperlinks.
- **REQ-098**: Self-update binary downloads must stream byte progress chunks, driving a Ratatui `Gauge` progress widget in the TUI and a progress bar component in the Web Hub.
- **REQ-099**: The terminal execution runner must register panic hooks and signal handlers (`SIGTERM`, `SIGHUP`) to guarantee terminal raw mode cleanup and cursor restoration on unexpected process termination.
- **REQ-100**: The terminal presentation layer must wrap Ratatui behind clean traits (`TerminalSurface`, `TerminalComponent`, `PromptEditor`, `ModalView`), isolating application logic from raw Ratatui/textarea struct layouts and enabling seamless headless unit testing.
- **REQ-101**: Resuming from process suspension (`SIGTSTP`) must trigger `terminal.clear()` and immediately redraw the active inline viewport, preventing shell prompt artifacts from persisting in the viewport area.
- **REQ-102**: Rapid terminal window resize bursts must debounce in the event loop, collapsing duplicate resize events to render a single flicker-free frame once geometry stabilizes.
- **REQ-103**: When user scrollback is offset upwards during active streaming, automatic scroll-to-bottom must pause (engaging an auto-scroll lock with a visual down-indicator) until manually scrolled back to the bottom or dismissed with `G`/`End`.
- **REQ-104**: In-browser Iroh P2P networking in `crates/rho-wasm` must feed incoming byte frames directly into canonical `RpcEvent` signal channels in Rust WebAssembly without traversing `JsValue` or `serde_wasm_bindgen` boundaries.

## Cross-cutting consolidation (Slice 0–3 leftovers → Slice 5)

- **REQ-105**: Relative-time formatting (`format_relative_time`) must live in `rho-ui-core` and derive its second/minute/hour/day buckets from a duration-humanizing crate (`humantime`/`timeago`) instead of the hand-rolled ladder in `src/ui/render/formatters.rs`; the TUI session modal, session picker, and Web Hub session sidebar consume the same function.
- **REQ-106**: Footer path display (`abbreviate_home` → `~` prefix) and git-branch detection (`get_git_branch`) must live in the `rho-ui-core` footer module, retaining the `git` subprocess fallback; `footer/mod.rs` and the `cli/rpc.rs` callers re-export through it.
- **REQ-107**: Session-tree rendering (`build_tree_display` + `render_tree_ascii`) must live in `rho-ui-core` as a shared tree renderer so the TUI ASCII projection and the Web Hub projection consume one model; `src/ui/interactive/tree_view/` is deleted.
- **REQ-108**: Tool/runtime duration formatting must be a single `format_duration`/`format_duration_ms` in `rho-ui-core`; `format_elapsed` (`src/ui/interactive/layout/widget.rs`) and the inline `elapsed.as_millis()` in `src/cli/mcp.rs` are removed.
- **REQ-109**: Truncation and right-alignment helpers (`truncate_with_ellipsis`, `fit_right_aligned`, `sanitize_status_text`) must be consolidated into the same `rho-ui-core` text module as `truncate_to_width`/`visible_width`, eliminating the `src/ui/interactive/footer/text.rs` copies.
- **REQ-110**: `ModelRegistry` must be the single provider-metadata source of truth: the hardcoded `MODEL_CONTEXT_WINDOWS` table and the `gpt-6-astra` per-provider special-case in `crates/rho-harness-core/src/tokens/mod.rs` are deleted and folded into `ModelRegistry` (completes REQ-020).
- **REQ-111**: YAML frontmatter parsing for skills and prompt templates must be a single function in `rho-harness-core` (adopt `serde_yaml` or one hand-rolled parser), consumed by both `crates/rho-harness-core/src/prompts/template.rs` and `crates/rho-harness-core/src/skills/parser.rs`.
- **REQ-112**: The keymap reducer (`map_key`, `map_app_action`, `parse_key_chord`) must live in `rho-ui-core`, relying on crossterm's own key parsing and deleting the hand-rolled `SINGLE_CHAR_KEYS` table in `src/ui/interactive/key_parser.rs`.

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

## Open questions

- **Shared-helper home / dependency direction**: `rho-ui-core` currently has no dependency on `rho-harness-core`, yet REQ-087/105/106/107/108/109/110 need numeric, relative-time, footer, and tree helpers shared with `rho-harness-core` and `rho-engine`. Decide whether `rho-ui-core` gains a `rho-harness-core` dependency (allowed by the decoupling invariant, which forbids only transport/renderer deps) and re-exports, or the helpers move to a lower crate. Owner: implementer of Slice 5; matters because it dictates re-export direction and prevents re-duplication.

## Codebase audit (2026-09-15)

Quantified findings from a full codebase survey, organized by category.
Items marked **NEW** were not in the original spec draft.

### Scale of the UI layer

| Area | Files | Lines |
|------|-------|-------|
| `src/ui/` | 155 | 20,392 |
| `src/repl/` | 114 | 14,278 |
| `src/repl/live/` alone | 65 | 10,033 |
| **Total presentation** | **269** | **34,670** |

Directory depth: 138 files at depth 4, 57 files at depth 5. AGENTS.md targets <= 4-5 levels.

### Confirmed redundancies (with verified line counts)

| # | Item | Location | Lines | Status |
|---|------|----------|-------|--------|
| 1 | Hand-rolled fuzzy scoring | `src/repl/interactive/fuzzy.rs` | 112 | `fuzzy-matcher` (`SkimMatcherV2`) already in workspace; modal code (`src/ui/interactive/state/modal/mod.rs`) already uses it. Completion code (`args.rs`, `mod.rs`) still uses the hand-rolled version. |
| 2 | Custom LCS diff engine | `src/ui/render/diff.rs` | 397 | `similar` crate NOT yet in deps. |
| 3 | Manual word wrapping math | `src/ui/interactive/layout/text.rs` | 216 | Ratatui `Paragraph::wrap` replaces this. |
| 4 | Custom markdown line scanning | `src/ui/markdown/line.rs` + `spacing.rs` | 276 | Regex-based parsing; `pulldown-cmark` replaces. |
| 5 | Custom ANSI markdown stream compiler | `src/ui/markdown/renderer.rs` + `stream.rs` + `elements.rs` | 824 | Manual escape sequence tracking and word boundary regexes. |
| 6 | Duplicate tool card formatting | `src/ui/interactive/transcript/tool.rs` + `src/ui/render/card.rs` | 289 | Two implementations of the same tool card rendering. |
| 7 | Event batching queues | `src/ui/interactive/events/batch.rs` + `src/repl/live/batch.rs` | 397 | `PendingUiBatch` (194 lines) and `LiveBatch` (203 lines) still exist. |
| 8 | Standalone session picker | `src/ui/interactive/session_picker/` | 251 | 136 + 115 lines (tests). |
| 9 | Threaded input reader | `src/repl/input_reader/` | 320 | 4 files (worker, paused, mod, tests). `crossterm::event::EventStream` replaces. |
| 10 | Custom screen simulator | `src/ui/interactive/controller/tests/screen_sim.rs` | 1,142 | Ratatui `TestBackend` replaces. |
| 11 | Editor state micro-fragmentation | `src/ui/interactive/state/editor/` | 483 | 5 files (geometry, history, mutate, navigation, mod). `ratatui-textarea` replaces. |
| 12 | Layout module fragmentation | `src/ui/interactive/layout/` (non-test) | 1,823 | 14 files including 4 modal sub-files. |
| 13 | Controller module fragmentation | `src/ui/interactive/controller/` (non-test) | 1,244 | 11 source files + 11 test files at depth 5. |
| 14 | ANSI painting + cursor diffing | `controller/paint.rs` + `ansi.rs` | 207 | Hand-rolled terminal diffing. |
| 15 | OutputTracker cursor bookkeeping | `controller/output.rs` | 54 | Manual cursor position tracking. |
| 16 | System message timer | `controller/system_message.rs` | 28 | Custom 3-second expiration logic. |
| 17 | Render cache | `controller/cache.rs` | 146 | Dual-slot transcript render cache (+ 272 lines of tests). |
| 18 | Custom box-drawing | `src/ui/block/` | 531 | 3 files (mod, wrap, tests). Ratatui `Block`+`Borders` replaces. |
| 19 | Chrome manual dividers | `src/ui/interactive/layout/chrome.rs` | 159 | Hand-rolled ANSI escape divider formatting with hardcoded escape codes. |
| 20 | Idle vs turn dual loop | `src/repl/live/idle/` + `turn/` | 2,503 | Fragmented into 13 files across two directories. |
| 21 | Duplicate command dispatch | `src/repl/live/message.rs` + `src/repl/line_mode/dispatch.rs` | 975 | 720 + 255 lines handling overlapping slash commands. |
| 22 | RPC daemon handlers | `src/cli/rpc.rs` | 1,272 | 20+ `handle_*` functions duplicating session command logic. |
| 23 | Presenter hierarchy | `broadcast_presenter.rs` + `rpc_presenter.rs` + `renderer/` | 630 | Three separate presenter/renderer implementations. |
| 24 | Thinking stream word-wrapper | `src/ui/render/renderer/thinking.rs` | 228 | Manual word-boundary tracking for thinking output. |
| 25 | `indicatif` spinner wrapper | `src/ui/render/renderer/activity.rs` | 54 | Single external dep for progress spinner. |
| 26 | `inquire` usage | 10 call sites across `src/cli/auth/`, `src/repl/commands/`, `src/repl/live/` | ~150 | External CLI prompting library; should use in-TUI modals. |
| 27 | `reedline` usage | `src/repl/line_mode/`, `src/repl/completer.rs`, `src/repl/prompt.rs`, `src/repl/interactive/history.rs` | ~350 | Line-mode editor + completer + prompt + history. |
| 28 | `anstyle` usage | ~20 references across `render/`, `block/`, `theme/` | scattered | Intermediate styling crate; Ratatui `Style` replaces. |
| 29 | Redundant `crossterm::terminal::size()` | 7 call sites | scattered | `src/ui/block/mod.rs`, `src/repl/line_mode/shell.rs`, `src/cli/runner.rs`, `src/ui/markdown/table/mod.rs`, `src/ui/markdown/line.rs`, `src/ui/render/formatters.rs`, `src/ui/render/renderer/mod.rs` |
| 30 | Markdown highlight (color quantization) | `src/ui/markdown/highlight.rs` | 170 | Contains color downsampling; Ratatui `Color::Rgb` replaces. |
| 31 | Markdown table manual borders | `src/ui/markdown/table/` | ~269 | Ratatui `Table` widget replaces. |

### NEW: Duplicated functions across crate boundaries

| # | Function | Location A | Location B | Issue |
|---|----------|-----------|-----------|-------|
| 32 | `format_tokens()` | `src/ui/interactive/footer/text.rs` | `crates/rho-engine/src/engine/metrics/types.rs` | **Different thresholds** (10k vs 100k for switching format) — subtle behavioral divergence. |
| 33 | `tool_title_style()` | `src/ui/render/preview.rs` (returns `anstyle::Style`) | `src/ui/theme/mod.rs` (returns `crossterm Style`) | Two implementations using different style crates for the same visual concept. |

### NEW: Micro-fragmented files (< 50 lines, non-test)

Files too small to justify their own module:
- `src/ui/stream.rs` (18 lines)
- `src/repl/commands/mcp.rs` (20 lines)
- `src/platform/suspend.rs` (23 lines)
- `src/repl/live/modal/interaction/prompt.rs` (25 lines)
- `src/repl/prompt.rs` (27 lines)
- `src/ui/interactive/controller/system_message.rs` (28 lines)
- `src/repl/live/navigation/clipboard.rs` (30 lines)
- `src/ui/interactive/tree_view/ascii.rs` (30 lines)
- `src/repl/input_reader/paused.rs` (34 lines)
- `src/repl/live/idle/editor.rs` (35 lines)
- `src/repl/completer.rs` (36 lines)
- `src/repl/interactive/sources.rs` (37 lines)
- `src/ui/interactive/keymap/map.rs` (39 lines)
- `src/ui/interactive/keymap/chord.rs` (40 lines)
- `src/repl/commands/args.rs` (43 lines)
- `src/repl/commands/thinking.rs` (44 lines)
- `src/ui/interactive/state/editor/history.rs` (47 lines)
- `src/ui/interactive/transcript/types.rs` (47 lines)
- `src/ui/render/presenter/sink.rs` (14 lines)

### NEW: Web Hub JS/CSS surface

| File | Lines |
|------|-------|
| `www/hub/wasm/rho_wasm.js` (generated) | 1,285 |
| `www/hub/js/app.js` | 511 |
| `www/hub/js/session.js` | 458 |
| `www/hub/js/client.js` | 187 |
| `www/hub/js/auth.js` | 130 |
| `www/hub/js/registry.js` | 48 |
| `www/hub/css/hub.css` | 960 |
| **Total hand-written JS + CSS** | **2,294** |

### NEW: Duplicate word-wrapping state machines

`StreamWordWrapper` (`src/ui/markdown/stream.rs`, 366 lines) and `ThinkingStreamTracker` (`src/ui/render/renderer/thinking.rs`, 228 lines) implement the **same character-by-character word-wrapping algorithm** with nearly identical struct fields:

| Field | `StreamWordWrapper` | `ThinkingStreamTracker` |
|-------|--------------------|-----------------------|
| `col` | ✓ | ✓ |
| `pending_spaces` / width | ✓ | ✓ |
| `pending_word` / width | ✓ | ✓ |
| ANSI tracking (`active_ansi`, `pending_ansi`) | ✓ | ✗ |
| `at_line_start` | ✗ | ✓ |

Ratatui's `Paragraph::wrap` and styled `Span` rendering eliminate both entirely.

### NEW: Footer text helpers (`truncate_to_width` / `visible_width` already consolidated)

`visible_width()` and `truncate_to_width()` are now single definitions in `src/ui/block/wrap.rs` (Slice 0 done). The remaining fragmentation is footer-specific text math in `src/ui/interactive/footer/text.rs`: `truncate_with_ellipsis`, `fit_right_aligned`, and `sanitize_status_text`, each re-implementing width measurement on top of `visible_width`/`truncate_to_width`. These join the shared `rho-ui-core` text module (REQ-109).

### NEW: `ANSI_PATTERN` regex shared across 12+ call sites

A static `LazyLock<Regex>` in `src/ui/block/wrap.rs` is used by 12+ files for ANSI-escape stripping. With Ratatui rendering, ANSI escape tracking disappears entirely — widgets work with typed `Style` and `Span`, not raw escape sequences.

### NEW: Hybrid markdown parsing — `pulldown-cmark` used partially

`pulldown-cmark` is already a workspace dependency (v0.13) and is used in `src/ui/markdown/elements.rs` for **inline** element rendering (bold, italic, strikethrough, links). But `src/ui/markdown/line.rs` (225 lines) does its own **block-level** parsing with manual string matching (`starts_with("```")`, `starts_with("#")`, regex for ordered lists). This hybrid approach means markdown is parsed by two different systems simultaneously.

### NEW: Custom LCS diff algorithm (not using `similar`)

`src/ui/render/diff.rs` (397 lines) implements a hand-rolled LCS table with backtracking (`build_lcs_table`, `backtrack_token_step`, `compute_token_diff`) plus a custom tokenizer (`char_category`-based). The `similar` crate is NOT in workspace deps. `similar` provides `TextDiff`, `ChangeTag`, and inline word-level diffing out of the box — would replace ~200 lines of algorithm code.

### NEW: Modal system fragmentation — 5,049 lines across 4 layers

The modal UI is split across four distinct layers:

| Layer | Files | Lines |
|-------|-------|-------|
| Modal state machine | `src/ui/interactive/state/modal/` (2 files) | 202 |
| Modal layout rendering | `src/ui/interactive/layout/modal/` (4 files) | 592 |
| Modal interaction key handling | `src/repl/live/modal/interaction/` (4 files) | 466 |
| Per-modal constructors + handlers | `src/repl/live/modal/` (8 files) | 1,613 |
| Modal tests | 11 test files | 2,176 |
| **Total** | **29 files** | **5,049** |

Each modal (model, login, mcp, session, settings, tree, remote, help) follows the same pattern: `open_*_selector()` builds a `ModalState`, `handle_*_key()` processes input. A unified `use_modal()` hook in `rho-ui-core` would define the state machine + key handling once, with each modal only providing its option list and selection callback.

### NEW: Tab-delimited string packing in model selector

`src/repl/live/modal/model.rs` packs provider, active mark, default mark, and description into a tab-delimited string (`"{}\t{}\t{}\t{}"`), then unpacks via `d.split('\t').next()`. A typed `ModelItem` struct in `rho-ui-core` eliminates this fragile encoding.

### NEW: Custom `TerminalBackend` trait — parallel to Ratatui's

`src/ui/interactive/controller/backend.rs` (96 lines) defines a `TerminalBackend` trait with `CrosstermBackend` wrapping `crossterm` directly. Ratatui provides its own `Backend` trait and `CrosstermBackend` that does the same thing with double-buffered diffing built in.

### NEW: Hand-rolled syntax highlighting color quantization

`src/ui/markdown/highlight.rs` (170 lines) includes `syntect_color_to_ansi16()` — a custom RGB-to-ANSI16 quantizer with grayscale detection, dominant-channel heuristics, and brightness thresholds. Ratatui supports `Color::Rgb(r, g, b)` natively in terminals with TrueColor, eliminating the need for quantization entirely.

### Items already cleaned up (spec claims confirmed)

- **Global statics** (`ACTIVE_APPROVALS`, `ACTIVE_STEERING`, `REMOTE_PROMPT_QUEUE`): not found in codebase — already removed.
- **Presenter hierarchy names** (`BroadcastPresenter`, `RpcPresenter`, `TerminalRenderer`): still exist. `BroadcastPresenter` at 257 lines, `RpcPresenter` at 172 lines, `TerminalRenderer` at 201 lines.
- **`TranscriptRenderCache` / `CachedItemRender`**: not found by name — may have been renamed to `controller/cache.rs` (146 lines).
- **`ThinkingStreamTracker`**: exists inside `renderer/thinking.rs` (228 lines).

### Total deletable surface estimate & line-savings reality

A common question is whether unification saves "25k lines of code". It is critical to distinguish **gross deletable/replaceable lines** from **net lines saved**:

1. **Gross Deletion vs. Net Savings**:
   - The ~23,600 line estimate represents **gross legacy surface area touched, deleted, or rewritten**.
   - Replacing hand-rolled code with modern crate ecosystems still requires writing typed domain models, custom hooks, Dioxus components, and Ratatui widgets:
     - `crates/rho-ui-core`: **+3,130 lines**
     - `crates/rho-wasm`: **+2,041 lines** (replacing vanilla JS and custom CSS)
     - `src/repl/runner.rs`: **+1,877 lines** (replacing ~10,344 lines of fragmented idle/turn loops)
     - `src/ui/` (editor, modal, terminal, widgets): **+2,900 lines**
   - Writing ~10,000–12,000 lines of replacement code means the ultimate **net codebase reduction** is around **~10,000 to 12,000 lines**, never 25,000 net lines.

2. **Completed Deletions (Phase 1 & 2)**:
   - `src/repl/live/`: **-10,344 lines** across 65 files
   - `www/hub/js/`: **-1,334 lines** across 5 files
   - `src/ui/interactive/controller/`: **-3,096 lines** across 21 files (replaced by Ratatui modal & TestBackend)
   - `src/ui/interactive/session_picker/`: **-251 lines** across 2 files (migrated to Ratatui `StandardModalView`)
   - `src/ui/interactive/controller/tests/screen_sim.rs`: **-1,142 lines**
   - `src/ui/render/diff.rs`: **-397 lines** (replaced by `similar` in `rho-ui-core`)
   - `src/ui/interactive/events/batch.rs`: **-328 lines**
   - `src/repl/input_reader/`: **-320 lines**
   - `src/repl/interactive/fuzzy.rs`: **-112 lines**
   - `src/ui/stream.rs`: **-18 lines**
   - **Current totals**: **19,996 lines deleted**, **15,570 lines added** (Net reduction: **~4,426 lines** to date).

3. **Phase 3 Path A Target (Remaining Legacy Surface)**:
   - `src/ui/interactive/layout/` (22 files, 2,735 lines): To be replaced by direct Ratatui layout / widget projections.
   - `src/ui/interactive/state/` (11 files, ~1,500 lines): To be replaced by `rho-ui-core` signals.
   - `src/ui/block/` (3 files, 531 lines): To be replaced by Ratatui `Block`/`Borders`.
   - `src/ui/markdown/` (17 files, 3,398 lines) & `src/ui/render/` (21 files, 3,504 lines): To be streamlined into `rho-ui-core` block projections.
   - Completing Path A will eliminate an additional ~5,000–8,000 lines of gross legacy code, bringing final net savings to ~10,000–12,000 lines.

## Out of scope

- Converting static landing and documentation pages (`www/index.html`, `www/docs.html`, `www/packages.html`) to Dioxus.
- Developing a native desktop GUI (e.g., Dioxus desktop with wry/tao).
- Rewriting the CLI non-interactive mode (`--print`, batch script execution).
