# Repository instructions

## Code structure

- Prioritize cohesion and natural readability over artificial file length limits.
  Files up to ~300-400 lines are healthy and encouraged when they keep related
  logic, data types, and tests together.
- Avoid micro-fragmentation: do not shatter straightforward logic, argument
  definitions, or tiny helpers into separate 10-50 line files or deep directory
  hierarchies. Keep directory trees shallow (target <= 4-5 levels deep).
- Prefer idiomatic Rust function signatures: use natural parameter lists (up to
  6 arguments) or explicit domain config/context structs. Never pack parameters
  into tuples (`(a, b): (&str, &str)`) to circumvent argument count thresholds.
- Prefer standard in-file `#[cfg(test)] mod tests` for unit tests. Use a sibling
  `tests.rs` only when a test suite is genuinely distinct or very large. Do not
  create nested test directories (`tests/sub/mod.rs`) for unit tests.

## Lint policy

- Do not add Clippy `allow`, `expect`, command-line exclusions, or crate-level
  lint suppressions. Refactor code to satisfy the configured lints instead.
- Remove any existing Clippy suppression encountered in code being changed.
- Verify with `make clippy` (or `cargo clippy --workspace --all-targets -- -D warnings`).

## Testing and performance

- Use `cargo test --workspace` for test feedback during development (a bare
  `cargo test` only covers the root `rho` package, not the other crates).
- Write unit tests in in-file `#[cfg(test)] mod tests` blocks or sibling `tests.rs`
  files, avoiding deep nested test module folders.
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
