# Implementation Plan: Block Output and Activity Spinner Padding

Apply 1-space horizontal padding across block card outputs, thinking blocks, and activity spinners, aligning rho's visual presentation with pi.dev's `outputPad` model.

## User-Observable Changes

1. **Card/Block Outputs**: Tool execution cards, user messages, running tool widgets, and notices gain 1 character of horizontal padding inside their colored background cards (content is inset by 1 column, wrapped at `width - 2`, and padded by at least 1 column on the right).
2. **Activity Spinner**: The working/activity spinner in both interactive TUI and headless CLI output is indented with a 1-space leading pad (` ⠋ Working...`).
3. **Thinking Block**: Thinking block output in transcripts and CLI streams is indented with a 1-space leading pad per line (` analyzing the problem...`).

## Slices

### Slice 1: BlockFormat Horizontal Padding (Effort: 2)

- **Goal**: Implement 1-space left/right padding and inner-width wrapping inside `BlockFormat`.
- **Files**:
  - `src/ui/block/mod.rs`
  - `src/ui/block/tests.rs`
- **Tasks**:
  1. Define `const HORIZONTAL_PADDING: usize = 1;` in `src/ui/block/mod.rs`.
  2. In `render_plain`, `render_styled`, and `render_line`, compute `inner_width = self.width.saturating_sub(HORIZONTAL_PADDING * 2).max(1)`.
  3. In `padded_line`, prepend `HORIZONTAL_PADDING.min(self.width)` spaces with background style before `content`, and adjust trailing spaces to `self.width.saturating_sub(HORIZONTAL_PADDING.saturating_add(visible))`. For empty lines (e.g. top/bottom vertical padding), ensure full width is filled with background spaces.
  4. Update existing tests in `src/ui/block/tests.rs` to assert 1-space left pad (`line.starts_with("\x1b[40m ")`), `width - 2` wrap points, and exact total width invariant.
- **Verification**: `cargo test ui::block`

### Slice 2: Activity Spinner 1-Space Left Padding (Effort: 2)

- **Goal**: Add a 1-space left padding to activity spinner rendering across interactive and headless modes.
- **Files**:
  - `src/ui/interactive/layout/chrome.rs`
  - `src/ui/render/renderer/activity.rs`
  - `src/ui/interactive/layout/tests/activity.rs`
- **Tasks**:
  1. In `src/ui/interactive/layout/chrome.rs` (`working_line_text`), format full string with leading space: `format!(" {accent}{spinner}{reset} {dim}{label}{reset}")`.
  2. In `src/ui/render/renderer/activity.rs` (`start_spinner`), update template to `" {spinner:.cyan} {msg} {elapsed:.dim}"`.
  3. In `src/ui/interactive/layout/tests/activity.rs`, update assertions to verify leading space before spinner glyph.
- **Verification**: `cargo test ui::interactive::layout::tests::activity`

### Slice 3: Thinking Block 1-Space Left Padding (Effort: 1)

- **Goal**: Add 1-space left padding to thinking block lines in `format_thinking_block`.
- **Files**:
  - `src/ui/render/formatters.rs`
  - `src/ui/render/tests/formatters.rs`
- **Tasks**:
  1. In `src/ui/render/formatters.rs` (`format_thinking_block`), format each line as `format!("{d} {line}{d:#}\n")`.
  2. In `src/ui/render/tests/formatters.rs`, update assertions to verify ` {line}` formatting.
- **Verification**: `cargo test ui::render::tests::formatters`

### Slice 4: Full Workspace Verification & Documentation (Effort: 2)

- **Goal**: Ensure all layout, transcript, and controller tests pass with no regressions, formatting is clean, and clippy warnings are zero.
- **Tasks**:
  1. Run `cargo test --workspace` and update any transcript / screen simulation test fixtures that assert exact column positions for blocks or spinners.
  2. Run `cargo fmt --all -- --check`.
  3. Run `make clippy`.
- **Verification**: `cargo fmt --all -- --check && make clippy && cargo test --workspace`
