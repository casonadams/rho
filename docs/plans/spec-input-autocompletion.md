# Specification: User Input Autocompletion Engine

**Status**: Approved
**Target Milestone**: v0.10.x
**Affected Crates**: `rho`, `rho-harness-core`

---

## 1. Problem Statement

In interactive REPL mode, `rho` currently provides rudimentary slash-command matching and basic `@` file matching. However:
1. **Replacement Inaccuracy**: Applying completion (`apply_selected_completion`) makes brittle assumptions about whitespace and string prefixes rather than respecting the exact replacement token range (`Range<usize>`) calculated by the completer.
2. **Directory Chaining**: When selecting a directory (e.g. `@src/` or `/`), `rho` closes autocomplete rather than immediately chaining and re-querying the contents of the chosen subdirectory (a signature capability of `oh-my-pi`).
3. **Mid-Prompt Skill and Mention Triggers**: Skills cannot easily be triggered mid-prompt with `/skill:<name>`, and model mention completions (e.g., `^` or model tags) or structured prompt action completions do not exist.
4. **Stale Asynchronous Completions**: Dynamic file enumeration or MCP argument lookups lack clean cancellation and prefix boundary matching.

## 2. Requirements and Goals

1. **Range-Based Token Replacement**:
   - `apply_selected_completion` must use the exact `replacement: Range<usize>` associated with the selected `Completion` item.
   - When a completion is inserted, the cursor must be placed immediately after the inserted replacement text.
2. **Directory Chaining (Auto-Reopen)**:
   - When completing a directory path ending with `/` or `\\`, autocomplete must not terminate. Instead, it must immediately refresh and show the children within that directory.
3. **Slash Command Chaining**:
   - When completing a slash command that accepts arguments (such as `/model`, `/login`, `/skill`), accepting the command name with a trailing space must automatically re-trigger argument autocompletion for that command.
4. **Mid-Prompt Skill Invocations**:
   - Typing `/skill:` mid-sentence or mid-prompt must trigger skill autocompletion, replacing only the `/skill:...` token rather than wiping the preceding prompt.
5. **Quality & Performance Standards**:
   - Maintain cognitive complexity <= 15 on all functions.
   - All tests must complete within milliseconds.
   - 100% verified via `make all`.
