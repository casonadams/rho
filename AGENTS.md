# Repository instructions

## Lint policy

- Do not add Clippy `allow`, `expect`, command-line exclusions, or crate-level lint suppressions. Refactor code to satisfy the configured lints instead.
- Remove any existing Clippy suppression encountered in code being changed.

## Completion

- Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` before finishing.
