# Implementation Plan: Inline Ghost-Text Input Prediction & Word Completion

Based on `docs/plans/spec-ghost-text-prediction.md`, this plan implements inline ghost-text predictions in `rho` as in `oh-my-pi`.

---

## Slice 1: EditorState Ghost Text Representation & Layout Rendering
- **Changes**:
  - Extend `EditorState` in `src/ui/interactive/state/editor/mod.rs` with `ghost_text: Option<String>`, `set_ghost_text()`, `clear_ghost_text()`, `accept_ghost_text() -> bool`.
  - In `src/ui/interactive/layout/editor.rs`, update `wrap_editor` / `render_editor_lines` to render the dim ghost text (`\x1b[2m<ghost>\x1b[0m`) directly following the cursor cell without moving the software or hardware cursor.
  - Add unit tests for `EditorState::accept_ghost_text()` and ghost text layout formatting.
- **Verification**: `cargo test -p rho --lib ui::interactive::layout::tests::editor`

## Slice 2: Prediction Provider & Key Dispatch
- **Changes**:
  - Add `predict_next_text(text: &str, history: &InteractiveHistory, completions: &CompletionSet) -> Option<String>` in `src/repl/interactive/prediction.rs`.
  - In the REPL event loop (`src/repl/live/idle/dispatch.rs`), recalculate ghost text on edit actions.
  - When pressing <kbd>→</kbd> (or `InputAction::Edit(UiAction::MoveRight)`), if the editor cursor is at the end of the text and ghost text is present, accept the ghost text instead of a no-op move right.
  - Add unit tests for history and command prediction and right-arrow acceptance.
- **Verification**: `cargo test -p rho --lib repl::live`

## Slice 3: Quality Gate & Verification
- Verify formatting, clippy, cognitive complexity (`cccc`), quality delta (`ripwire`), and CRAP coverage gating via `make all`.
- Commit atomically following conventional commit standards.
