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
- Replace the imperative JavaScript Web Hub frontend with a Dioxus-based WebAssembly application utilizing Dioxus Components.
- Introduce a shared Rust UI presentation crate (`crates/rho-ui-core`) powered by Dioxus reactivity (`dioxus-core` / `dioxus-signals`) that encapsulates:
  - Reactive signals (`Signal<T>`) for transcripts, prompt editor, modal stack, and active tool states.
  - Reusable custom hooks (`use_session`, `use_modal`, `use_autocomplete`, `use_streaming_transcript`).
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
  - Exposes fine-grained reactive state (`Signal<Transcript>`, `Signal<ModalState>`, `Signal<PromptInput>`) and custom hooks.
  - Exposes deterministic state reducers for user input, streaming RPC events, tool execution states, and modal navigation.
- **Terminal UI (`src/ui/` & `src/repl/`)**:
  - Implemented with Ratatui using `ratatui::Viewport::Inline(height)`.
  - Driven by a headless Dioxus runtime loop that triggers Ratatui redraws whenever signals mutate or coroutines emit events.
  - Prompt editing powered by `ratatui-textarea`, supporting standard input mode or modal Vim mode (Normal, Insert, Visual, Replace, motions, operators) toggled via settings.
  - Renders active turn progress, editor buffer, autocomplete popup, and modal overlays using standard Ratatui widgets reading from Dioxus signals.
  - Prints finalized turn outputs into stdout scrollback once completed, erasing the inline viewport and restoring normal terminal flow.
- **Web Hub (`www/hub/` & `crates/rho-wasm`)**:
  - Full single-page application written in Rust using Dioxus and Dioxus Components.
  - Directly drives Iroh P2P connections and RPC sessions via `rho-wasm`.
  - Uses identical state machines from `rho-ui-core` to render fleet views, session sidebars, chat transcripts, modals, and tool output cards.
  - Deployed as a compiled WebAssembly binary + minimal HTML entry point (`index.html`).

## Architecture

```
+-------------------------------------------------------------+
|                      crates/rho-ui-core                     |
|           [Dioxus Reactive Core: Signals & Hooks]           |
|                                                             |
|  - Reactive Signals: Transcript, Modals, Input, ActiveTool  |
|  - Hooks: use_session(), use_modal(), use_autocomplete()    |
|  - Shared Event Actions, Token Streaming & Parsers          |
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
|  - Headless Runtime   |             |                       |
|  - stdout scrollback  |             |                       |
+-----------------------+             +-----------------------+
```

## Requirements

### Architecture and Reactive Core
- **REQ-001**: A new workspace crate `crates/rho-ui-core` must be established, compiling cleanly on both native targets and `wasm32-unknown-unknown`.
- **REQ-002**: `rho-ui-core` must implement UI state management using Dioxus reactive signals (`Signal<T>`), memos, and custom hooks (`use_session`, `use_modal`, `use_autocomplete`).
- **REQ-003**: The modal state machine in `rho-ui-core` must support unified navigation semantics (up/down, enter to select, escape to dismiss, search filtering, digit jump keys) shared across TUI and Web.
- **REQ-004**: State mutations and streaming events must update signals directly, ensuring both TUI and Web run the identical state transition logic without translation layers.

### Ratatui Terminal Interface
- **REQ-005**: The terminal interactive runner must use Ratatui with `Viewport::Inline` to render the bottom interactive area (prompt editor, autocomplete menu, active tool progress, and modals).
- **REQ-006**: The prompt input buffer must be managed by `ratatui-textarea`, deprecating bespoke cursor, geometry, and kill-ring logic in `src/ui/interactive/state/editor/`.
- **REQ-007**: The prompt editor must support a configurable Vim mode (Normal, Insert, Visual, Replace, motions `h`/`j`/`k`/`l`/`w`/`b`/`$`/`^`, operators `d`/`y`/`c`, undo/redo) following the `ratatui-textarea` transition state machine.
- **REQ-008**: The inline viewport height must dynamically resize based on active contents (input line count, open modal height, or autocomplete list) without overflowing terminal boundaries.
- **REQ-009**: When an execution turn finishes, the completed turn content (user prompt, assistant response, and tool summaries) must be written directly to terminal scrollback, leaving the terminal ready for the next prompt.
- **REQ-010**: Terminal keybinding semantics must remain identical: `Escape` cancels/dismisses, `Ctrl+C` clears input drafts, `Ctrl+D` exits when prompt is empty, and standard arrow/vi keys navigate modals.
- **REQ-011**: Custom ANSI painting and cursor diffing logic in `src/ui/interactive/controller/paint.rs` and `ansi.rs` must be completely removed.

### Dioxus Web Hub Interface
- **REQ-012**: All JavaScript application logic in `www/hub/js/` must be replaced by a Dioxus application compiled to WebAssembly.
- **REQ-013**: The Dioxus application must render the Fleet view, Active Node workspace, Session sidebar, Chat transcript, and Modals using Dioxus Components.
- **REQ-014**: Peer-to-peer connectivity via Iroh must remain direct in-browser, integrated into the Dioxus component lifecycle via asynchronous hooks/signals.
- **REQ-015**: Local storage persistence (node tickets, saved sessions, sidebar toggle states) must be managed via web-sys wrappers within the Dioxus application.
- **REQ-016**: The Web Hub build pipeline must integrate into `make wasm`, producing a production-ready WASM bundle and asset structure.

### UI Parity and Interaction
- **REQ-017**: All modals (thinking selector, model picker, login provider, MCP servers, session list) must present the same options, indicators (active checkmarks), and search filtering in both interfaces.
- **REQ-018**: Thinking blocks must stream in real time and support expandable/collapsible accordion display on both platforms.
- **REQ-019**: Tool execution cards must visually represent status states (running, success, error) consistently across both TUI and Web Hub.

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
