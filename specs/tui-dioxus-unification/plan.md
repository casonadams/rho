# Implementation Plan: Ratatui TUI and Dioxus Web UI Unification

## Overview

This plan defines a vertical-slice migration replacing `rho`'s hand-rolled ANSI terminal diffing engine and vanilla JavaScript Web Hub with:
1. **`crates/rho-ui-core`**: A shared reactive state core powered by Dioxus signals and custom hooks.
2. **Native TUI**: Ratatui with `Viewport::Inline` and `ratatui-textarea` (featuring optional Vim mode).
3. **Web Hub**: A Dioxus 0.6+ WebAssembly application built with Dioxus Components over Iroh P2P.

---

## Slice 1: Reactive Core (`crates/rho-ui-core`)

- **Goal**: Create a platform-agnostic Rust crate containing unified UI view models, Dioxus reactive signals, and state hooks that compile on native and `wasm32-unknown-unknown`.
- **Acceptance Criteria**:
  - `crates/rho-ui-core` compiles cleanly on `cargo check --target wasm32-unknown-unknown` and native.
  - View models and Semantic UI Block IR exist (`ContentBlock`, `InlineSpan`, `DiffHunk`, `StyleToken`, `ModalState`, `AutocompleteCandidate`).
  - Markdown, code fences, diffs, tables, diagrams, and thinking blocks are parsed once into `ContentBlock` structures.
  - `StreamChunkParser` extracts streaming tokens, `<thinking>` tags, duration timestamps, and tool parameters into strongly-typed `StreamEvent`s.
  - Reusable hooks (`use_session`, `use_modal`, `use_permission_prompt`, `use_autocomplete`, `use_stream_parser`) pass in-memory headless unit tests.
- **Tasks**:
  1. (Effort: 2) Add `crates/rho-ui-core` to the root `Cargo.toml` workspace with dependencies on `dioxus-core`, `dioxus-signals`, `pulldown-cmark`, `similar`, `fuzzy-matcher`, and `serde`.
  2. (Effort: 3) Implement Semantic UI Block IR (`ContentBlock`, `InlineSpan`, `DiffHunk`, `StyleToken`, `ThemeTokens`) representing formatted text, code fences, diffs, tables, and diagrams.
  3. (Effort: 3) Implement unified markdown, table, and stream tokenizer converting streams into typed `ContentBlock` items.
  4. (Effort: 2) Implement unified diff tokenizer via `similar` crate in `rho-ui-core`, producing `DiffHunk` structures and deprecating custom LCS logic.
  5. (Effort: 3) Implement `StreamChunkParser` emitting `StreamEvent`s for content deltas, `<thinking>` blocks, and tool executions.
  6. (Effort: 3) Implement `use_modal` state machine handling selection index, `fuzzy-matcher` scoring, active indicators, and pagination.
  7. (Effort: 3) Implement `use_permission_prompt` state machine handling tool confirmation flow (Allow, Always, Deny, Edit), custom denial reasons, and prefilled command mutations.
  8. (Effort: 2) Implement `SlashCommandDef` registry and `CompletionEngine` for slash commands (`/`), skills, models, and file paths using `fuzzy-matcher`.
  9. (Effort: 2) Implement `PromptHistory` state tracking (`previous`, `next`, draft preservation) shared between TUI and Web Hub.
  10. (Effort: 2) Implement `FooterMetrics` (token counts, context %, cost, tokens/sec), `SessionTreeState`, and `WelcomeDisplay` in `rho-ui-core`.
  11. (Effort: 2) Implement `PROVIDER_DEFS` authentication metadata, `ModelItem` domain struct, `McpModalState`, `SkillModalState`, `SettingsState`, `SessionCommandExecutor`, and `use_session` lifecycle reducer (`Prompt`, `Steer`, `Abort`, `hydrate_messages`).
  12. (Effort: 2) Add table-driven unit tests for all state reducers and hooks.
- **Verification**:
  - `cargo test -p rho-ui-core`
  - `cargo check -p rho-ui-core --target wasm32-unknown-unknown`

---

## Slice 2: Ratatui Terminal Engine & Inline Viewport

- **Goal**: Replace hand-rolled ANSI line diffing and cursor tracking in the terminal with Ratatui's native `Viewport::Inline`.
- **Acceptance Criteria**:
  - Inline terminal interaction retains full scrollback history; completed turns are committed to terminal stdout without viewport overlap.
  - Hand-rolled cursor bookkeeping (`OutputTracker`, `src/ui/interactive/controller/paint.rs`, `screen_sim.rs`) is deprecated and removed.
  - Headless test backend (`ratatui::backend::TestBackend`) verifies rendering deterministically without custom terminal emulation.
  - Process suspension (`Ctrl+Z` / `SIGTSTP`) and non-TTY piped mode cleanly bypass or resume Ratatui viewport without terminal state corruption.
- **Tasks**:
  1. (Effort: 2) Add `ratatui` (with `crossterm` feature) to workspace dependencies.
  2. (Effort: 3) Implement Ratatui `Terminal<CrosstermBackend>` initialization with `Viewport::Inline(height)` dynamically derived from active content.
  3. (Effort: 3) Connect Dioxus headless reactive runtime to Ratatui draw loop, triggering frame renders upon signal mutations or terminal resize events.
  4. (Effort: 2) Implement scrollback turn completion writer that prints finalized user prompt and assistant output into stdout history, clearing the active inline viewport.
  5. (Effort: 2) Collapse presenter adapters (`BroadcastPresenter`, `RpcPresenter`, `TerminalRenderer`) into a single event stream feeding `rho-ui-core`.
  6. (Effort: 3) Port terminal controller unit tests from the custom `screen_sim.rs` to Ratatui `TestBackend`.
  7. (Effort: 2) Delete obsolete ANSI diffing, table formatting, markdown line regexes, and batching code in `src/ui/interactive/controller/paint.rs`, `ansi.rs`, `src/ui/block/`, `src/ui/markdown/table/`, `src/ui/markdown/line.rs`, `src/ui/markdown/spacing.rs`, `src/ui/render/card.rs`, `src/ui/interactive/layout/text.rs` (word wrapping math), `src/ui/interactive/events/batch.rs`, `src/repl/live/batch.rs` (`LiveBatch`), `src/repl/completer.rs`, `src/repl/prompt.rs`, and `screen_sim.rs`.
- **Verification**:
  - `cargo test -p rho --lib ui::interactive`

---

## Slice 3: Prompt Editor & Vim Mode (`ratatui-textarea`)

- **Goal**: Replace ~620 lines of hand-crafted editor logic with `ratatui-textarea`, providing robust multi-line editing, undo/redo, and configurable Vim mode.
- **Acceptance Criteria**:
  - Text input supports multi-line navigation, word skipping, undo/redo, and paste without custom cursor math.
  - Large pastes automatically collapse into markers (`[paste #1 +50 lines]`) expanding on submit, and clipboard image pasting inserts token markers.
  - Configurable Vim mode (`Normal`, `Insert`, `Visual`, `Replace`) supports standard motions (`h`/`j`/`k`/`l`/`w`/`b`/`$` etc.) and operators (`d`/`y`/`c`).
  - Active editor mode (e.g. `[NORMAL]`, `[INSERT]`) renders cleanly on the input divider.
- **Tasks**:
  1. (Effort: 1) Add `ratatui-textarea` to workspace dependencies.
  2. (Effort: 3) Implement `Vim` state transition machine and key dispatcher in the interactive editor following `ratatui-textarea/examples/vim.rs`.
  3. (Effort: 2) Integrate `TextArea` widget into the Ratatui inline viewport layout with theme styling and placeholder support.
  4. (Effort: 2) Integrate bracketed paste interception for collapsed paste markers (`PasteStore`) and clipboard image insertion into `ratatui-textarea`.
  5. (Effort: 2) Add configuration option in `config.toml` (`[editor] mode = "vim" | "default"`).
  6. (Effort: 2) Write unit tests for Vim mode transitions, motions, text deletion, collapsed paste markers, and undo/redo stacks.
  7. (Effort: 2) Delete `src/ui/interactive/state/editor/` (`geometry.rs`, `history.rs`, `mutate.rs`, `navigation.rs`) and prune redundant low-level editor bindings in `src/ui/interactive/keybinding_loader/`.
- **Verification**:
  - `cargo test -p rho --test editor`

---

## Slice 4: Ratatui Modals, Permission Screens & Widgets

- **Goal**: Implement standard Ratatui stateful widgets for user selection screens, permission approval dialogs, autocomplete dropdowns, and streaming tool cards driven by `rho-ui-core` signals.
- **Acceptance Criteria**:
  - All interactive selectors (`/thinking`, `/model`, `/login`, `/mcp`, `/session`) render centered over the inline viewport using Ratatui `Clear`, `Block`, and `List`.
  - Tool permission approval screens (`Allow once`, `Allow always`, `Deny with reason`, `Edit command`) render with scrollable command previews and seamless transition to embedded `ratatui-textarea` editing.
  - Slash command and file autocomplete popup renders anchored above or inside the input area with fuzzy highlight matching.
  - Active tool running states and thinking status accordions render cleanly in the inline view.
- **Tasks**:
  1. (Effort: 2) Build reusable `render_modal` helper using Ratatui `Clear`, `Block`, borders, and `ListState`.
  2. (Effort: 3) Implement modal views for thinking level, model selector, auth provider, MCP management, skill explorer (`/skill`), and session history.
  3. (Effort: 3) Implement tool permission approval screen with scrollable diff/command preview and embedded `ratatui-textarea` for command modification.
  4. (Effort: 2) Build autocomplete popup widget anchored to current cursor position using `rho-ui-core` candidates.
  5. (Effort: 2) Build active tool card and streaming spinner widget driven directly by `Signal<ActiveToolState>`, deleting bespoke channel polling in `src/repl/live/bash_runner/progress.rs`.
  6. (Effort: 2) Unit test modal and permission widget rendering across terminal dimensions using `TestBackend`.
  7. (Effort: 2) Delete legacy `src/ui/render/diff.rs`, `src/ui/render/preview.rs` (redundant detectors), and `src/repl/interactive/fuzzy.rs`.
- **Verification**:
  - `cargo test -p rho --lib repl::live`

---

## Slice 5: Dioxus Web Hub Application

- **Goal**: Rebuild `www/hub/` as a single-page WebAssembly application in Rust using Dioxus and Dioxus Components, completely eliminating vanilla JavaScript.
- **Acceptance Criteria**:
  - All JavaScript files in `www/hub/js/` are removed; `www/hub/index.html` loads the compiled Dioxus WASM binary.
  - Fleet node grid, session sidebar, chat transcript, thinking accordions, permission approval cards, and modals render using Dioxus Components (`dioxuslabs.com/components`).
  - Markdown renders with full feature fidelity: syntax-highlighted code blocks with copy buttons, HTML tables, interactive SVG Mermaid diagrams, and formatted diffs.
  - Direct in-browser Iroh P2P connection operates reactively via Dioxus coroutines and `rho-ui-core` signals.
  - `make wasm` produces an optimized release WASM bundle.
- **Tasks**:
  1. (Effort: 2) Expand `crates/rho-wasm` to a Dioxus web target (`dioxus`, `dioxus-web`, and `dioxus-components`).
  2. (Effort: 3) Implement reactive Iroh client peer hook bridging P2P byte streams to `rho-ui-core` state signals using strongly-typed `RpcCommand` and `RpcEvent`.
  3. (Effort: 3) Build Fleet Overview view (node grid, node cards, connection status pills, ticket pairing modal).
  4. (Effort: 3) Build Workspace Chat view (chat transcript, markdown rendering, tool execution cards, thinking accordion).
  5. (Effort: 3) Build Modals, Session Sidebar, Status Bar, and Session Export (session history, conversation branch tree, auth modal, MCP server manager, skill explorer, model picker, provider credentials, in-browser Markdown/HTML export, `StatusBar` component reading `Signal<FooterMetrics>`) using Dioxus Components.
  6. (Effort: 2) Connect web storage persistence (saved nodes, tickets, active session ID) via browser `web-sys` hooks.
  7. (Effort: 2) Remove `www/hub/js/*.js` and update `Makefile` target `make wasm` with `wasm-opt` size optimization.
- **Verification**:
  - `make wasm`
  - Browser verification of node pairing, chat streaming, and modal workflows.

---

## Slice 6: Parity Audit, Cleanup & Final Polish

- **Goal**: Verify complete bidirectional feature parity between native terminal and Web Hub, verify zero lint regressions, and update documentation.
- **Acceptance Criteria**:
  - All modals, commands, and tool displays look and behave identically in TUI and Web Hub.
  - All workspace crates pass strict Clippy with zero warnings (`-D warnings`).
  - Documentation in `docs/` and `README.md` accurately describes the new architecture and Vim mode option.
- **Tasks**:
  1. (Effort: 2) Audit keyboard navigation and visual parity between TUI and Web Hub across all modals.
  2. (Effort: 2) Remove obsolete dependencies (`inquire`, `indicatif`, `reedline`, `anstyle`) from root `Cargo.toml`, and remove leftover deprecated structs and files.
  3. (Effort: 2) Update documentation: document Vim mode keybindings and Web Hub architecture.
  4. (Effort: 2) Run full workspace format check, clippy, and test suite.
- **Verification**:
  - `cargo fmt --all -- --check`
  - `make clippy`
  - `make wasm`
  - `cargo test --workspace`
