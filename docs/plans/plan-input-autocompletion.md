# Implementation Plan: User Input Autocompletion Engine

Based on the specification in `docs/plans/spec-input-autocompletion.md`, this plan details vertical slices for implementing advanced autocompletion in `rho` inspired by `oh-my-pi`.

---

## Slice 1: Range-Accurate Completion Replacement in REPL Live Autocomplete
- **Problem**: `apply_selected_completion` in `src/repl/live/autocomplete.rs` manually splits text with `starts_with('/')` or `text[cursor..].find(' ')` instead of utilizing the `replacement: Range<usize>` stored in `Completion` and selected in `AutocompleteState`.
- **Changes**:
  - Update `AutocompleteItem` in `src/ui/interactive/state/autocomplete.rs` to retain the target replacement range `Range<usize>`.
  - Refactor `apply_selected_completion` to replace `text[range.start..range.end]` with `val`, placing the cursor at `range.start + val.len()`.
  - Add comprehensive unit tests verifying cursor placement and prefix preservation across arbitrary prompt locations.
- **Verification**: `cargo test -p rho --lib repl::live::autocomplete`

## Slice 2: Directory and Slash Command Chaining
- **Problem**: Selecting a directory completion (`src/`) closes the autocomplete popup. In `oh-my-pi`, selecting a directory chains into subdirectory completions without requiring another trigger.
- **Changes**:
  - Check if the accepted completion is a directory (ends with `/` or `\\`).
  - In `handle_accept_key`, if chaining is appropriate, re-run `update_autocomplete_state_generic` immediately so the autocomplete stays open with the next level of suggestions.
  - Add unit tests verifying that accepting a directory keeps the autocomplete open with child paths.
- **Verification**: `cargo test -p rho --lib repl::live::autocomplete`

## Slice 3: Mid-Prompt Skill and Slash Command Completion
- **Problem**: In `src/repl/interactive/completion.rs`, `complete_slash_commands` requires `prefix.starts_with('/')`, meaning typing `/skill:` mid-prompt does nothing or fails.
- **Changes**:
  - Support mid-prompt `/skill:` completions where the trigger starts at `prefix.rfind("/skill:")` or whitespace-preceded `/skill:`.
  - Calculate `replacement` range specifically for the skill token.
  - Unit tests for mid-prompt `/skill:` and multi-token queries.
- **Verification**: `cargo test -p rho --lib repl::interactive::tests`

## Slice 4: Full Quality Gate & Cleanup
- Run `make all` (fmt, clippy, cccc cognitive complexity <= 15, ripwire quality delta, test coverage / CRAP score <= 30).
- Create atomic conventional commits for each feature slice.
