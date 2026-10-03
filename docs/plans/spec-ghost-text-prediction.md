# Specification: Inline Ghost-Text Input Prediction & Word Completion

**Status**: Approved
**Target Milestone**: v0.10.x
**Affected Crates**: `rho`

---

## 1. Problem Statement

In `oh-my-pi`, when typing in the prompt editor, users receive non-intrusive inline text prediction rendered as dim "ghost text" immediately after the cursor. Users can press <kbd>→</kbd> (Right Arrow) or <kbd>Tab</kbd> to accept the prediction (or a word of it).
In `rho`:
1. Autocompletion is only provided via a dropdown popup (`AutocompleteState`).
2. There is no inline ghost-text hint mechanism to predict upcoming words, history completions, or slash command continuations.
3. Users have to type out repetitive prompts or open full popup menus.

## 2. Requirements and Goals

1. **Inline Prediction State (`ghost_text`)**:
   - `EditorState` retains an optional `ghost_text: Option<String>` representing the pending inline suggestion.
   - When the cursor is at the end of the line (or prompt), and ghost text is present, it is rendered in dim text (`\x1b[2m<ghost>\x1b[0m`) immediately after the cursor.
2. **Prediction Sources**:
   - **History Prefix Matching**: Substrings matching recent commands/prompts in `InteractiveHistory`.
   - **Slash Commands & Subcommands**: When typing `/` commands (e.g. `/mod` -> `el`), suggest remaining command and argument templates.
3. **Acceptance Keybindings**:
   - When ghost text is active and cursor is at the end of the buffer:
     - <kbd>→</kbd> (Right Arrow): Accepts the full ghost-text prediction into the editor.
     - Any regular typing updates or invalidates the ghost text cleanly.
4. **Performance & Cleanliness**:
   - Ghost text generation must be synchronous, in-memory, and take < 1ms.
   - Zero terminal flickering; ghost text is included in editor layout calculations without moving the hardware cursor.
   - Cognitive complexity <= 15 on all functions.
   - Verified via `make all`.
