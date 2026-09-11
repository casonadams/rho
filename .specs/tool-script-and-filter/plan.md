# Implementation Plan: Tool Batching and In-Engine Output Filtering (`script`)

Implement the `script` tool enabling single-turn tool batching with in-engine regex line filtering and surrounding context expansion across all platforms.

## User-Observable Changes

1. **New `script` Tool**: Models can execute an array of tool steps in a single turn (`script(steps: [...])`).
2. **Output Filtering**: Adding `"filter": "pattern"` to any step extracts only matching lines, dramatically slashing returned tokens.
3. **Surrounding Context Lines**: Matches include surrounding context lines (defaulting to 2 lines before/after, matching `grep -C 2`), with automatic merging of overlapping matches.
4. **Clean Unix Pipe UI**: Interactive and headless cards display filtered steps as `tool args | grep -C 2 "pattern"`.
5. **Calm Spinner Invariant**: The activity spinner stays untouched as ` ⠋ Working...`; live running tool status renders in the widget card above the editor.

## Slices

### Slice 1: Pure-Rust Line Filter Engine (Effort: 2)

- **Goal**: Implement high-performance, bounded line-by-line regex filtering with context expansion and span merging.
- **Files**:
  - `crates/rho-engine/src/tools/script/filter.rs`
  - `crates/rho-engine/src/tools/script/tests.rs`
- **Tasks**:
  1. Create `filter_lines(content: &str, pattern: &str, context: usize) -> Result<String, FilterError>`.
  2. Compile `pattern` using `regex::RegexBuilder` with case-insensitivity (smart-case).
  3. Scan lines and collect matching line numbers (0-indexed).
  4. If matches is empty, return `"[No lines matched filter: \"...\"]"`.
  5. Expand each match index $i$ to $[i - \text{context}, i + \text{context}]$.
  6. Merge overlapping or immediately adjacent intervals into contiguous spans.
  7. Format output: matching lines prefixed with line number and `:`, context lines with `-`, and disjoint spans separated by `--\n`.
  8. Add table-driven unit tests for match-only (`context: 0`), standard context (`context: 2`), boundary conditions, and invalid regexes.
- **Verification**: `cargo test --package rho-engine tools::script::tests::filter`

### Slice 2: Arguments Schema & Catalog Registration (Effort: 2)

- **Goal**: Define `ScriptArgs` and `ScriptStep` with JSON schema serialization and catalog prompt declarations.
- **Files**:
  - `crates/rho-harness-core/src/args.rs`
  - `crates/rho-engine/src/tools/builtin_tools/catalog.rs`
  - `crates/rho-engine/src/tools/builtin_tools/mod.rs`
- **Tasks**:
  1. Define `ScriptStep` and `ScriptArgs` in `crates/rho-harness-core/src/args.rs`:
     - `tool: String`
     - `args: serde_json::Value`
     - `filter: Option<String>`
     - `context: Option<usize>`
  2. Implement `schemars::JsonSchema` and add docstrings.
  3. Add `BuiltinToolDeclaration` for `script` in `catalog.rs` with guidelines explaining batching and filtering.
  4. Ensure `normalize_schema` cleans the generated schema.
- **Verification**: `cargo test --package rho-harness-core args`

### Slice 3: Script Execution Engine & Tool Dispatcher (Effort: 3)

- **Goal**: Execute steps sequentially, invoke tools in-process, apply line filters, and short-circuit on errors.
- **Files**:
  - `crates/rho-engine/src/tools/script/mod.rs`
  - `crates/rho-engine/src/tools/script/runner.rs`
  - `crates/rho-engine/src/tools/builtin_tools/mod.rs`
- **Tasks**:
  1. Implement `ScriptTool` holding a tool registry / dispatcher capable of invoking built-in and dynamic tools.
  2. For each step:
     - Check permissions and execute the tool.
     - If output is text and `filter` is set, pass through `filter_lines`.
     - Record step output with header: `[Step K: <tool> | grep -C <N> "<filter>"]`.
  3. Short-circuit if any step fails or returns an error, preserving prior successful steps and reporting the failing error.
  4. Enforce `output_max_bytes` ceiling across accumulated script outputs.
  5. Add unit and integration tests executing multi-step scripts (`read` + `web_fetch` with filters).
- **Verification**: `cargo test --package rho-engine tools::script`

### Slice 4: UI Formatting for Widget and Transcript (Effort: 2)

- **Goal**: Render active and completed scripts using Unix pipe syntax in `RunningToolWidget` and `BlockFormat` transcript cards without altering the spinner.
- **Files**:
  - `src/ui/interactive/transcript/tool.rs`
  - `src/ui/interactive/layout/widget.rs`
  - `src/ui/interactive/transcript/tests/tool.rs`
- **Tasks**:
  1. Update `format_tool_header` in `src/ui/interactive/transcript/tool.rs` to format `script` steps using `| grep -C <N> "<filter>"`.
  2. Render multi-step cards cleanly inside `BlockFormat` with step enumeration and match reduction summary.
  3. In `src/ui/interactive/layout/widget.rs`, render running step progress (e.g. `script (1/2) web_fetch ... | grep ...`) with streaming tail output.
  4. Add transcript unit tests asserting pipe rendering.
- **Verification**: `cargo test ui::interactive::transcript::tests::tool`

### Slice 5: Workspace Verification, Documentation & Benchmarks (Effort: 2)

- **Goal**: Verify workspace health, zero clippy warnings, and document the feature.
- **Files**:
  - `docs/tools.md`
  - `www/docs.html`
- **Tasks**:
  1. Document `script` tool and regex filter usage in `docs/tools.md` and `www/docs.html`.
  2. Run `cargo fmt --all -- --check`.
  3. Run `make clippy`.
  4. Run `cargo test --workspace`.
- **Verification**: `cargo fmt --all -- --check && make clippy && cargo test --workspace`
