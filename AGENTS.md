# Repository instructions

## Code structure

- Keep files concise (~150 lines target). Treat growth beyond ~150 lines as a
  signal to check cohesion and split along natural architectural seams when it
  clarifies responsibilities.
- Separate unit tests into sibling `tests.rs` or `tests/` submodules rather than
  embedding large `#[cfg(test)]` blocks inside production source files when
  files grow beyond ~150 lines.
- Avoid premature fragmentation: do not break straightforward logic into tiny,
  artificially separated helpers that obscure control flow.

## Lint policy

- Do not add Clippy `allow`, `expect`, command-line exclusions, or crate-level
  lint suppressions. Refactor code to satisfy the configured lints instead.
- Remove any existing Clippy suppression encountered in code being changed.
- Verify with `make clippy` (or `cargo clippy --workspace --all-targets -- -D warnings`).

## Testing and performance

- Use `cargo test --workspace` for test feedback during development (a bare
  `cargo test` only covers the root `rho` package, not the other crates).
- Place unit tests in a dedicated `tests.rs` or `tests/` file
  (`#[cfg(test)] mod tests;`) to keep production implementation files concise
  and cleanly separated from test harnesses.
- In HTTP client builders, always configure `.no_proxy()` or reuse static client
  singletons (`HttpClient` / `LazyLock`), and use `rustls-tls-webpki-roots`.
  Never build unconfigured `reqwest::Client` instances in hot paths or test
  fixtures to prevent macOS `SCDynamicStoreCopyProxies` IPC lockups in parallel
  test threads.
- In token counting, reuse static `CoreBPE` instances via `LazyLock` rather than
  calling `tiktoken_rs::cl100k_base()` repeatedly.
- In `build.rs` scripts, always provide absolute or workspace-anchored paths for
  `cargo:rerun-if-changed` to prevent Cargo from invalidating incremental build
  caches on every invocation.

## UX and modal guidelines

- Standardize all interactive selectors on the clean `/thinking` modal pattern:
  - Construct in-TUI popups using `ModalState` (`src/repl/live/modal/`) rather than suspending raw mode to run external CLI prompts (`inquire`).
  - Title: Clear, concise Title Case (e.g., `"Select Thinking Level"`, `"Login Provider"`, `"Settings"`).
  - Subtitle: Default to empty string `""` to avoid visual clutter in the modal frame.
  - Option Labels: Format with fixed-width alignment (e.g. `format!("{id:14}")`) so descriptions line up vertically.
  - Option Descriptions: Keep text concise and append an active indicator (`"  ✓"`) if the option is currently active or configured.
  - Navigation: Support standard controls across all selectors: `Up`/`Down` arrows, `k`/`j`, `Tab`/`Shift+Tab`, `Enter` to select, and `Esc`/`Ctrl+C` to cancel. Support digit jump keys (`1..=9`) where lists are fixed and short.
  - Search: Enable `.with_search(true)` on open-ended or longer lists so typing immediately fuzzy-filters candidates.
- Keybinding semantics:
  - `Escape`: Cancel/interrupt running execution, or dismiss active modals.
  - `Ctrl+C`: Clear the current input draft or filter query (never interrupts running turns or kills the process).
  - `Ctrl+D`: Exit when the prompt is empty.

## Completion

- Run `cargo fmt --all -- --check`, `make clippy` (or `cargo clippy --workspace --all-targets -- -D warnings`), and
  `cargo test --workspace` before finishing.
