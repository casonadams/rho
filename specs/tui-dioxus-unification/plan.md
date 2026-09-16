# Implementation Plan: Ratatui TUI and Dioxus Web UI Unification

## Overview

This plan defines a vertical-slice migration replacing `rho`'s hand-rolled ANSI terminal diffing engine and vanilla JavaScript Web Hub with:
1. **`crates/rho-ui-core`**: A shared reactive state core powered by Dioxus signals and custom hooks.
2. **Native TUI**: Ratatui with `Viewport::Inline` and `ratatui-textarea` (featuring optional Vim mode).
3. **Web Hub**: A Dioxus 0.6+ WebAssembly application built with Dioxus Components over Iroh P2P.

### Codebase audit summary & progress status (2026-09-15)

Total presentation layer baseline: **269 files, 34,670 lines** (`src/ui/` 20,392 + `src/repl/` 14,278).
Gross legacy code touched/deletable: **~23,600 lines**.

#### Line Savings Reality: Gross vs. Net
Replacing hand-rolled infrastructure with modern crate ecosystems requires writing typed domain models, custom hooks, Dioxus components, and Ratatui widgets (~12,000–14,000 lines added). The projected final **net reduction is ~10,000 to 12,000 lines** (not 25,000 net lines).

#### Progress to Date:
- **Deletions completed**: **25,892 lines deleted** across 128 files (`src/repl/live/` -10,344, `src/ui/interactive/layout/` -4,554, `src/ui/interactive/controller/` -3,096, `www/hub/js/` -1,334, `screen_sim.rs` -1,142, `state/editor/` -520, `runner.rs` cleanup -420, `diff.rs` -397, `batch.rs` -328, `input_reader/` -320, `session_picker/` -251, `color quantization` -104, `fuzzy.rs` -112, `stream.rs` -18).
- **Additions (modern architecture)**: **16,472 lines added** (`rho-ui-core` +3,130, `rho-wasm` +2,041, `runner.rs` +1,450, Ratatui modals/editor/widgets +2,980).
- **Net reduction to date**: **9,420 lines**. `src/ui/` + `src/repl/` went from **269 files / 34,670 lines** to **143 files / 18,590 lines**.
- **Remaining legacy surface (Path A)**: `state/` (~1,000 lines), `block/` (~560 lines), `markdown/` (~3,300 lines), `render/` (~3,500 lines). Completing these brings net reduction to ~10,000–12,000 lines.

See `spec.md` § "Codebase audit" for full inventory.

---

## Slice 0: Pre-migration consolidation (no new deps)

Quick wins that reduce surface area before the Ratatui/Dioxus migration begins. Each is independent and can land as a standalone PR.

- **Goal**: Eliminate confirmed duplications, consolidate micro-files, and fix the `format_tokens` divergence without introducing new framework dependencies.
- **Acceptance Criteria**:
  - `fuzzy.rs` deleted; all completion code uses `fuzzy-matcher::skim::SkimMatcherV2`.
  - Single `format_tokens()` in `rho-harness-core` or `rho-engine`, used by both UI footer and engine metrics.
  - Single `tool_title_style()` in `src/ui/theme/mod.rs`, `src/ui/render/preview.rs` calls it.
  - Redundant `crossterm::terminal::size()` calls consolidated into a shared helper.
  - All tests pass, clippy clean.
- **Tasks**:
  1. (Effort: 1) Delete `src/repl/interactive/fuzzy.rs`; update `completion/args.rs` and `completion/mod.rs` to use `SkimMatcherV2`. Invert score polarity (skim returns higher=better, hand-rolled was lower=better).
  2. (Effort: 1) Consolidate `format_tokens()`: keep the engine version (richer formatting), delete `src/ui/interactive/footer/text.rs` copy, re-export from a shared location.
  3. (Effort: 1) Delete `tool_title_style()` from `src/ui/render/preview.rs`; callers use `Theme::tool_title_style()` instead.
  4. (Effort: 1) Add `fn terminal_width() -> u16` helper in `src/ui/` or `rho-harness-core`; replace 7 direct `crossterm::terminal::size()` calls.
  5. (Effort: 1) Merge micro-files into parent modules where natural: `src/repl/input_reader/paused.rs` (34 lines) into `mod.rs`, `src/ui/render/presenter/sink.rs` (14 lines) into `mod.rs`, `src/repl/live/idle/editor.rs` (35 lines) into `mod.rs`.
  6. (Effort: 1) Consolidate `visible_width()`: delete copies in `footer/text.rs` and `layout/text.rs`, keep the canonical one in `src/ui/block/wrap.rs`, re-export. Same for `truncate_to_width()` — keep one copy.
  7. (Effort: 2) Consolidate the two word-wrapping state machines: `StreamWordWrapper` (366 lines) and `ThinkingStreamTracker` (228 lines) share the same character-by-character algorithm with minor differences (ANSI tracking vs `at_line_start`). Extract a shared `ChunkWordWrapper` and parameterize the ANSI-tracking behavior.
- **Verification**:
  - `cargo test --workspace`
  - `make clippy`

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
  2. (Effort: 3) Implement Semantic UI Block IR (`ContentBlock`, `InlineSpan`, `HighlightedLine`, `DiffHunk`, `StyleToken`, `ThemeTokens`, `ImageAttachment`, `ToolInvocation`) and theme structures using `ratatui::style::Style` and `Color::Rgb` natively, eliminating `anstyle` from workspace dependencies.
  3. (Effort: 3) Implement unified markdown tokenizer converting streams into typed `ContentBlock` items, embedding OSC 8 hyperlinks for URLs and search results. **Audit note**: currently `pulldown-cmark` is used only for inline elements (`elements.rs`, 101 lines) while block-level parsing (`line.rs`, 225 lines) uses manual string matching and regex. This task unifies both into a single `pulldown-cmark` pipeline, deleting the hybrid approach.
  4. (Effort: 2) Add `similar` crate to workspace dependencies. Implement unified diff tokenizer via `similar` in `rho-ui-core`, producing `DiffHunk` structures and replacing the hand-rolled LCS table + backtracking (~200 lines of algorithm in `diff.rs`).
  5. (Effort: 3) Unify `UiEvent` and `RpcEvent` into a single canonical event schema in `rho_harness_core` and implement `StreamChunkParser` emitting canonical events.
  6. (Effort: 3) Implement `use_modal` state machine handling selection index, `fuzzy-matcher` scoring, active indicators, and pagination.
  7. (Effort: 3) Implement `use_permission_prompt` state machine handling tool confirmation flow (Allow, Always, Deny, Edit), custom denial reasons, and prefilled command mutations.
  8. (Effort: 2) Implement `SlashCommandDef` registry with `SlashArgumentType` and declarative `CompletionEngine` for slash commands (`/`), skills, models, and file paths using `fuzzy-matcher`.
  9. (Effort: 2) Implement `PromptHistory` state tracking (`previous`, `next`, draft preservation) shared between TUI and Web Hub.
  10. (Effort: 2) Implement `FooterMetrics` (token counts, context %, cost, tokens/sec), `format_tokens` and `format_size` utilities, `detect_supported_image_mime` sniffing with `fit_dimensions` downsampling, `RhoTicket` (standard ticket parsing and extraction), `SessionTreeState`, and `WelcomeDisplay` in `rho-ui-core`.
  11. (Effort: 2) Implement `PROVIDER_DEFS` authentication metadata, `RpcAuthBridge` callback integration, `ModelRegistry` capability descriptors, `McpModalState`, `SkillModalState`, `SettingsState`, `use_toast`, `SecretGuard` redaction, `WindowFocus` tracking, auto-scroll pinning with user scroll lock, `PromptQueueCoordinator`, `SessionCommandExecutor`, and `use_session` lifecycle reducer (`Prompt`, `Steer`, `Abort`, `hydrate_messages`).
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
  1. (Effort: 2) Add `ratatui` (with `crossterm` feature) to workspace dependencies and define trait abstraction layer (`TerminalSurface`, `TerminalComponent`, `ModalView`) in `src/ui/mod.rs`.
  2. (Effort: 3) Implement Ratatui `Terminal<CrosstermBackend>` initialization with `Viewport::Inline(height)` dynamically derived from active content, backed by a panic hook and `SIGTERM`/`SIGHUP` signal listener ensuring clean terminal restoration.
  3. (Effort: 3) Implement single unified async event loop in `src/ui/terminal.rs` with key-repeat event coalescing, standardized mouse wheel scrolling velocity, window resize debouncing, and SIGTSTP job suspension recovery (`terminal.clear()` on resume), replacing the fragmented dual `idle_loop` and `turn_loop` machinery (~2,500 lines across `src/repl/live/idle/` and `turn/`).
  4. (Effort: 2) Implement scrollback turn completion writer with OSC 133 semantic prompt marks (`OSC133_ZONE_START` / `OSC133_ZONE_END`) and terminal bell (`\x07`) notification on unfocused window, printing finalized user prompt and assistant output into stdout history and clearing the active inline viewport.
  5. (Effort: 2) Collapse presenter adapters (`BroadcastPresenter`, `RpcPresenter`, `TerminalRenderer`) into direct canonical event broadcast channel feeding `rho-ui-core`, routing all background engine warnings through `RpcEvent::Notice` to protect inline viewport rows from uncoordinated `eprintln!` writes.
  6. (Effort: 3) Port terminal controller unit tests from the custom `screen_sim.rs` to Ratatui `TestBackend`.
  7. (Effort: 2) Delete obsolete ANSI diffing, table formatting, markdown line regexes, stream wrappers, renderer state machines, session picker engine, fragmented idle/turn loops, thinking stream trackers, input reader threads, system message timers, dual-slot transcript caches, color quantization math, layout budget math, chrome divider formatters, and batching code.
     - **Completed Phase 1 & 2 Deletions**:
       - `src/ui/interactive/controller/` (all 21 files, 3,096 lines)
       - `src/ui/interactive/session_picker/` (251 lines) -> migrated `--resume` to Ratatui `StandardModalView`
       - `src/ui/interactive/events/batch.rs` (328 lines)
       - `src/ui/stream.rs` (18 lines)
       - `src/repl/live/` (65 files, 10,344 lines) -> unified into `src/repl/runner.rs`
       - `src/repl/input_reader/` (4 files, 320 lines)
       - `src/ui/interactive/controller/tests/screen_sim.rs` (1,142 lines)
       - `src/ui/render/diff.rs` (397 lines) -> replaced by `similar`
       - `src/repl/interactive/fuzzy.rs` (112 lines) -> replaced by `fuzzy-matcher`
       - `www/hub/js/` (5 files, 1,334 lines) -> replaced by Dioxus in `crates/rho-wasm`
       - `src/ui/interactive/layout/` (25 files, 4,554 lines) -> the whole hand-rolled layout engine. Removable once the
         REPL runner moved its selectors onto Ratatui `StandardModalView` + `run_modal_view`; `layout()`, `LayoutInput`,
         and `InteractiveLayout` had no production callers left. The one surviving helper, ANSI-aware `wrap_to_width`,
         moved into the canonical text module `src/ui/block/wrap.rs` together with its four behavior tests.
     - **Remaining Legacy for Path A Phase 3**:
       - `src/ui/interactive/state/` (11 files, ~1,500 lines) -> replace with `rho-ui-core` signals
       - `src/ui/block/` (3 files, ~560 lines) -> replace with Ratatui `Block`/`Borders` once the transcript projects
         `ContentBlock` instead of pre-rendered ANSI strings
       - `src/ui/markdown/` (17 files, 3,398 lines) -> fold into `rho-ui-core` parser + `ContentBlock`
       - `src/ui/render/` (21 files, 3,504 lines) -> fold into `rho-ui-core` block projections
- **Verification**:
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`

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
  2. (Effort: 3) Implement `PromptEditor` trait wrapping `ratatui-textarea` with `Vim` state transition machine and key dispatcher (Normal, Insert, Visual character/line selection, Replace, clipboard yanking/pasting via `arboard` with OSC 52 fallback) following `ratatui-textarea/examples/vim.rs`.
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
  2. (Effort: 3) Implement modal views for thinking level, model selector, auth provider, MCP management (displaying `McpTestReport` diagnostics), skill explorer (`/skill`), remote pairing (`/pair` with QR code), and session history, enforcing repository `AGENTS.md` modal UX guidelines (Title Case, fixed-width formatting, active indicators, digit jump keys `1..=9`).
  3. (Effort: 3) Implement tool permission approval screen with scrollable diff/command preview and embedded `ratatui-textarea` for command modification.
  4. (Effort: 2) Build autocomplete popup widget anchored to current cursor position using `rho-ui-core` candidates.
  5. (Effort: 2) Build active tool card, visual compaction milestone badges, streaming spinner widget, and self-update `Gauge` download progress bar driven directly by `Signal<ActiveToolState>`, deleting bespoke channel polling in `src/repl/live/bash_runner/progress.rs`.
  6. (Effort: 2) Unit test modal and permission widget rendering across terminal dimensions using `TestBackend`.
  7. (Effort: 2) Delete legacy `src/ui/render/diff.rs`, `src/ui/render/preview.rs` (redundant detectors), and `src/repl/interactive/fuzzy.rs`.
- **Verification**:
  - `cargo test -p rho --lib repl::live`

---

## Slice 5: Cross-cutting consolidation & crate adoption (Slice 0–3 leftovers)

- **Goal**: Finish the cross-cutting consolidation left by Slices 0–3 by moving the remaining hand-rolled formatters, provider metadata, tree rendering, frontmatter parsing, and the keymap reducer into `rho-ui-core`/`rho-harness-core` and adopting solid crates, so the Web Hub (Slice 6) and the parity pass (Slice 7) reuse shared implementations instead of re-rolling them.
- **Acceptance Criteria**:
   - `rho-ui-core` exposes single definitions for `format_duration`/`format_duration_ms`, `format_relative_time`, `abbreviate_home`/`get_git_branch`, the session-tree renderer, and the truncation/right-align helpers; all `src/` callers re-export through them.
   - `MODEL_CONTEXT_WINDOWS` and the `gpt-6-astra` per-provider special-case in `crates/rho-harness-core/src/tokens/mod.rs` are deleted; context windows resolve through `ModelRegistry` (completes REQ-020 / REQ-110).
   - `format_tokens`/`format_size` have a single canonical body in `rho-harness-core::tokens`; `rho-ui-core::state` and `rho-engine::tools::truncate` re-export it (duplicate bodies deleted).
   - Frontmatter parsing is one function in `rho-harness-core`; `prompts/template.rs` and `skills/parser.rs` call it.
   - The keymap reducer (`map_key`, `parse_key_chord`) lives in `rho-ui-core`; `SINGLE_CHAR_KEYS` is deleted and crossterm key parsing is used.
   - `humantime` powers relative-time buckets; no duplicate numeric/duration/relative-time formatters remain in `src/`.
   - `cargo test --workspace` and `make clippy` are clean.
- **Tasks**:
   1. (Effort: 1) Consolidate duration formatting into one `format_duration`/`format_duration_ms` in `rho-ui-core`; delete `layout/widget.rs` `format_elapsed` and the inline `elapsed.as_millis()` in `cli/mcp.rs`.
   2. (Effort: 1) Move `format_relative_time` to `rho-ui-core` and drive its sec/min/hour/day buckets with `humantime` (add to workspace deps); update `render/formatters.rs`, `ui/modal.rs`, `interactive/session_picker/mod.rs`, and `repl/live/modal/session.rs` to re-export.
   3. (Effort: 1) Move `abbreviate_home` + `get_git_branch` into the `rho-ui-core` footer module (keep the `git` subprocess fallback); re-export through `footer/mod.rs` and update the five `cli/rpc.rs` callers.
   4. (Effort: 2) Move `build_tree_display`/`render_tree_ascii` into `rho-ui-core` as a shared tree renderer (Web Hub reuses the projection; optional `tree-iterator` for the walk); delete `src/ui/interactive/tree_view/`.
   5. (Effort: 1) Consolidate truncation: move `truncate_with_ellipsis`, `fit_right_aligned`, and `sanitize_status_text` from `footer/text.rs` into the same text module as `truncate_to_width`/`visible_width` (`rho-ui-core`).
   6. (Effort: 1) Collapse `format_tokens`/`format_size` to a single body in `rho-harness-core::tokens`; re-export from `rho-ui-core::state` and `rho-engine::tools::truncate` (delete the duplicate bodies).
   7. (Effort: 2) Complete REQ-020: fold `MODEL_CONTEXT_WINDOWS` and the `gpt-6-astra` provider special-case from `crates/rho-harness-core/src/tokens/mod.rs` into the existing `ModelRegistry`; delete the table.
   8. (Effort: 1) Dedup frontmatter parsing into one function in `rho-harness-core` (adopt `serde_yaml` or a single hand-rolled parser); call it from `prompts/template.rs` and `skills/parser.rs`.
   9. (Effort: 2) Move the keymap reducer (`input/mod.rs` `map_key` + `key_parser.rs` `parse_key_chord`) into `rho-ui-core`; delete `SINGLE_CHAR_KEYS` and use crossterm key parsing; update `keybinding_loader`.
- **Dependency note**: Land this slice **before** Slice 6 so the Dioxus Web Hub consumes `rho-ui-core`'s `format_relative_time`, footer path/branch, tree renderer, and duration helpers rather than re-implementing them; Slice 7 then audits parity against these shared versions. `rho-ui-core` currently has no dependency on `rho-harness-core`, so shared numeric/tree helpers need a canonical home and a one-directional re-export (see spec Open questions).
- **Verification**:
   - `cargo test --workspace`
   - `make clippy`

---

## Slice 6: Dioxus Web Hub Application

- **Goal**: Rebuild `www/hub/` as a single-page WebAssembly application in Rust using Dioxus and Dioxus Components, completely eliminating vanilla JavaScript.
- **Acceptance Criteria**:
  - All JavaScript files in `www/hub/js/` are removed; `www/hub/index.html` loads the compiled Dioxus WASM binary.
  - Fleet node grid, session sidebar, chat transcript, thinking accordions, permission approval cards, and modals render using Dioxus Components (`dioxuslabs.com/components`).
  - Markdown renders with full feature fidelity: syntax-highlighted code blocks with copy buttons, HTML tables, interactive SVG Mermaid diagrams, and formatted diffs.
  - Direct in-browser Iroh P2P connection operates reactively via Dioxus coroutines and `rho-ui-core` signals.
  - `make wasm` produces an optimized release WASM bundle.
- **Tasks**:
  1. (Effort: 2) Expand `crates/rho-wasm` to a Dioxus web target (`dioxus`, `dioxus-web`, and `dioxus-components`).
  2. (Effort: 3) Implement direct in-WASM Iroh client peer hook in Rust bridging P2P byte streams to `rho-ui-core` state signals using strongly-typed canonical events without `JsValue` crossing or JS callbacks.
  3. (Effort: 3) Build Fleet Overview view (`use_fleet()` typed node store, node grid, node cards, connection status pills, ticket pairing modal).
  4. (Effort: 3) Build Workspace Chat view (chat transcript, markdown rendering, clickable search result hyperlinks, tool execution cards, thinking accordion).
  5. (Effort: 3) Build Modals, Session Sidebar, Status Bar, and Session Export (session history, session deletion/pruning, conversation branch tree, accessible auth modal with focus trapping, MCP server manager, skill explorer, model picker, provider credentials, in-browser Markdown/HTML export, `StatusBar` component reading `Signal<FooterMetrics>`) using Dioxus Components, deprecating bespoke CSS in `www/hub/css/hub.css`.
  6. (Effort: 2) Connect web storage persistence (saved nodes, tickets, active session ID) via browser `web-sys` hooks.
  7. (Effort: 2) Remove `www/hub/js/*.js` and update `Makefile` target `make wasm` with `wasm-opt` size optimization.
- **Verification**:
  - `make wasm`
  - Browser verification of node pairing, chat streaming, and modal workflows.

---

## Slice 7: Parity Audit, Cleanup & Final Polish

- **Goal**: Verify complete bidirectional feature parity between native terminal and Web Hub, verify zero lint regressions, and update documentation.
- **Acceptance Criteria**:
  - All modals, commands, and tool displays look and behave identically in TUI and Web Hub.
  - All workspace crates pass strict Clippy with zero warnings (`-D warnings`).
  - Documentation in `docs/` and `README.md` accurately describes the new architecture and Vim mode option.
- **Tasks**:
  1. (Effort: 2) Audit keyboard navigation and visual parity between TUI and Web Hub across all modals, and standardize interactive test fixtures on Ratatui's `TestBackend`.
  2. (Effort: 2) Remove obsolete dependencies (`inquire`, `indicatif`, `reedline`, `anstyle`) from root `Cargo.toml`, and remove leftover deprecated structs and files.
  3. (Effort: 2) Update documentation: document Vim mode keybindings and Web Hub architecture.
  4. (Effort: 2) Run full workspace format check, clippy, and test suite.
- **Verification**:
  - `cargo fmt --all -- --check`
  - `make clippy`
  - `make wasm`
  - `cargo test --workspace`
  - `tokei -e target`
