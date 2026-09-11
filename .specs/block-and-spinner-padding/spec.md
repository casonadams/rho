# Block Output and Activity Spinner Padding Spec

## Status

Approved

## Problem

Visual presentation in the terminal currently has alignment and padding inconsistencies across components:
1. **Block / Card Outputs**: Card-style block outputs (tool executions, tool call results, running tool widgets, notices, user message cards, and skill cards) currently have vertical top and bottom padding (one blank line above and below content), but zero horizontal padding. Text and ANSI-styled content inside blocks sit flush against the terminal's left edge (column 0), making cards feel cramped horizontally compared to their vertical breathing room.
2. **Activity Spinner**: The working/activity spinner indicator sits flush at column 0 in interactive and headless output (`⠋ Working...`), creating a harsh edge alignment compared to indented or bordered UI components.
3. **Thinking Blocks**: Collapsed/inline thinking blocks in `format_thinking_block` render flush at column 0 (`format!("{d}{line}{d:#}\n")`), visually breaking alignment with padded blocks and indented indicators.

In contrast, **pi.dev** (`@earendil-works/pi` / `@mariozechner/pi-coding-agent`) enforces a cohesive visual system via its `outputPad` setting (default: `1`):
- All boxed cards (`ToolExecutionComponent`, `UserMessageComponent`) use `new Box(paddingX = 1, paddingY = 1, bgFn)`: exactly 1 blank row above/below (`paddingY: 1`) and 1 space of horizontal padding left/right (`paddingX: 1`).
- Thinking blocks and spinners are left-padded by 1 column so the entire output stream shares a consistent 1-character left visual rail.

Bringing `rho`'s output padding into alignment with `pi.dev`'s model resolves these visual discrepancies.

## Users and stakeholders

- **Interactive REPL Users**: Benefit from visually balanced tool cards, thinking transcripts, and status spinners with consistent 1-character left margins.
- **Headless / CI Users**: Experience cleanly padded tool cards and progress spinners in non-interactive runs.

## Goals

- **Align `BlockFormat` with `pi.dev`'s `Box(1, 1)`**: Add 1-space horizontal padding (1 space left, at least 1 space right) inside all `BlockFormat` output, preserving the existing 1-line top and bottom vertical padding (`with_vertical_padding()`).
- **Constrain Text Wrapping**: Ensure text wrapping inside `BlockFormat` wraps within `width.saturating_sub(2).max(1)` columns so content never overflows the right margin.
- **Align Activity Spinner with `pi.dev`**: Add a 1-space left padding to the spinner indicator across interactive (`working_line_text`) and headless (`indicatif` progress spinner) renders (` ⠋ Working...`).
- **Align Thinking Block with `pi.dev`'s `outputPad`**: Add a 1-space left padding to thinking block lines in `format_thinking_block` (` {d}{line}{d:#}\n`) so thinking output aligns with cards and spinners.
- **Preserve Background Continuity**: Ensure full-width background color styling continues across all line fills, padding spaces, and vertical padding lines without ANSI reset bleeding.

## Non-goals

- Adding external unstyled margins or gutters *outside* the colored block background (the colored card background continues to span the full terminal width, matching `pi.dev`).
- Changing prompt line formatting (`>` or `edit>`) or modal dialogue padding.
- Forcing leading spaces onto streaming assistant markdown text (in `pi.dev`, this was requested to be toggleable via `outputPad: 0` in issues #2507 and #5436 to avoid copy-paste pollution; assistant streaming markdown remains flush at column 0 by default).
- Adding complex ASCII/Unicode box-drawing borders around tool blocks (like `┌─┐` / `│ │`); `pi.dev` uses background color fill boxes with padding (`Box(1, 1, bgFn)`), which matches `rho`'s `BlockFormat`.

## Current behavior vs. pi.dev

| Output Element | pi.dev Behavior (`outputPad: 1`) | `rho` Current Behavior | Proposed Alignment |
|---|---|---|---|
| **Tool Execution Card** | `Box(paddingX: 1, paddingY: 1, bg)` | `BlockFormat` (padX: 0, padY: 1) | `BlockFormat` (padX: 1, padY: 1) |
| **Running Tool Widget** | Boxed widget with 1-char inset | `BlockFormat` (padX: 0, padY: 1) | `BlockFormat` (padX: 1, padY: 1) |
| **User Message Card** | `Box(paddingX: 1, paddingY: 1, bg)` | `BlockFormat` (padX: 0, padY: 1) | `BlockFormat` (padX: 1, padY: 1) |
| **Activity Spinner** | Indented 1 space before spinner glyph | Flush at column 0 (`⠋ Working...`) | Indented 1 space (` ⠋ Working...`) |
| **Thinking Lines** | Indented 1 space per line | Flush at column 0 (`{d}{line}{d:#}`) | Indented 1 space (` {d}{line}{d:#}`) |
| **Assistant Markdown** | Indented 1 space (toggleable) | Flush at column 0 | Flush at column 0 (avoids copy-paste issues) |

### Current Code References in `rho`

1. **`src/ui/block/mod.rs`**:
   - `render_plain`, `render_styled`, and `render_line` wrap to `width.max(1)`.
   - `padded_line(&self, content: &str)` formats lines as `{style}{content}{style}{trailing_spaces}{reset}\n`, placing content immediately at column 0 with 0 horizontal padding.
2. **`src/ui/interactive/layout/chrome.rs:86`**:
   - `working_line_text` formats `{accent}{spinner}{reset} {dim}{label}{reset}` starting at column 0.
3. **`src/ui/render/renderer/activity.rs:41`**:
   - Progress bar template `{spinner:.cyan} {msg} {elapsed:.dim}` starts at column 0.
4. **`src/ui/render/formatters.rs:98`**:
   - `format_thinking_block` emits `{d}{line}{d:#}\n` starting at column 0.

## Desired behavior

1. **Block formatting (`BlockFormat`):**
   - Every content line inside `BlockFormat` begins with 1 space rendered in the block's background style (`{style} {content...}`).
   - Content wrapping wraps to `width.saturating_sub(2).max(1)` so that content does not overflow the right margin.
   - Trailing space fills the remainder of the line up to `width` in the block background style, guaranteeing at least 1 trailing space of padding when content reaches max wrapped width.
   - Empty padding lines generated by `.with_vertical_padding()` continue to fill the full `width` with spaces rendered in the block background style.
   - If available width is too narrow (`width <= 2`), wrapping falls back to 1 column gracefully without underflow.

2. **Activity Spinner indicator:**
   - In interactive mode, `working_line_text` renders a leading space before the spinner character: `" {accent}{spinner}{reset} {dim}{label}{reset}"`.
   - In headless mode, `TerminalRenderer::start_spinner` includes a leading space in the style template: `" {spinner:.cyan} {msg} {elapsed:.dim}"`.

3. **Thinking block formatting:**
   - In `format_thinking_block`, each line of thinking text is prepended with 1 space: `format!("{d} {line}{d:#}\n")`, matching `pi.dev`'s 1-space visual rail.

## Requirements

- **REQ-001**: `BlockFormat` must indent content lines by exactly 1 space on the left, styled with the block's background color.
- **REQ-002**: `BlockFormat` must wrap content lines to an inner width of `width.saturating_sub(2).max(1)` columns so that text never overflows the right padding boundary.
- **REQ-003**: `BlockFormat` must ensure the total visible width of every rendered line (including top/bottom padding lines and content lines) equals `width` (for `width >= 1`).
- **REQ-004**: `BlockFormat` must preserve active ANSI styles across wrapped line splits and restore the background style immediately after any inline style resets (`\x1b[0m`, `\x1b[49m`).
- **REQ-005**: Interactive working line layout (`working_line_text`) must prepend 1 space before the spinner glyph, producing `" {accent}{spinner}{reset} {dim}{label}{reset}"`.
- **REQ-006**: Truncation of the interactive working line (`truncate_to_width`) must account for the additional 1 column width of the leading space.
- **REQ-007**: Headless activity progress spinner (`TerminalRenderer::start_spinner`) must prepend 1 space before the spinner placeholder in its style template (`" {spinner:.cyan} {msg} {elapsed:.dim}"`).
- **REQ-008**: `format_thinking_block` must prepend 1 space before each line of thinking text, producing `"{d} {line}{d:#}\n"`.

## Invariants and security boundaries

- **Line length invariant**: For any line emitted by `BlockFormat` with target width $W > 0$, the visible character width (excluding ANSI escape sequences) must equal $W$.
- **ANSI hygiene**: Background color escape sequences must be properly reset at the end of each line (`\x1b[0m`) so styling does not bleed into subsequent terminal rows.
- **No terminal drift**: Cursor row calculations and height calculations in `InteractiveLayout` must remain exact; leading spaces do not alter the number of rows rendered.

## Definition of done

- Unit tests in `src/ui/block/tests.rs` verify 1-space left padding, right trailing padding, and wrapping at `width - 2`.
- Unit tests in `src/ui/interactive/layout/tests/activity.rs` verify that `working_line` contains the leading space before the spinner character.
- Unit tests in `src/ui/render/tests/formatters.rs` verify that `format_thinking_block` prepends a space to each line.
- Headless progress bar template in `src/ui/render/renderer/activity.rs` is updated to include the leading space.
- All existing layout and controller tests continue to pass with no regressions.
- `cargo fmt --all -- --check`, `make clippy`, and `cargo test --workspace` pass cleanly.

## Risks and mitigations

- **Narrow terminal windows:** If terminal width is 1 or 2 columns, `width.saturating_sub(2)` would evaluate to 0.
  - *Mitigation:* Use `.max(1)` on `inner_width` and clamp padding to available width (`1.min(width)`).
- **ANSI color bleeding on reset inside padded line:** An inner reset in user/tool content could reset the background color before trailing padding.
  - *Mitigation:* `wrap_styled_line` and `padded_line` already handle background re-application after resets; retain this logic with the 1-space prefix.
- **Visual line wrap truncation in widgets:** The running tool widget and transcript truncate output to `width.saturating_sub(4).max(1)`.
  - *Mitigation:* Ensure widget truncation and `BlockFormat` inner width do not double-clip content unexpectedly.
- **Copy-paste of code blocks:** Users copying text from terminal may include the leading space if applied to all assistant code output.
  - *Mitigation:* Following the lessons of `pi.dev` issues #2507 and #5436, keep assistant markdown streaming flush at column 0; only pad background cards (`BlockFormat`), thinking blocks, and the activity spinner.

## Out of scope

- Indenting user prompt input or command-line completions.
- Adding ASCII/Unicode borders (e.g. `┌─┐`) around blocks; background fill cards (`Box(1, 1, bg)`) are the standard pattern.
- Introducing a runtime configurable `outputPad` setting at this stage (can be added in a future settings iteration).
