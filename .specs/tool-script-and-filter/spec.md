# Tool Batching and In-Engine Output Filtering (`script`) Spec

## Status

Approved

## Problem

Multi-step workflows currently face two major performance and cost bottlenecks:
1. **Intermediate Context Bloat**: When an agent inspects large files or web documents (e.g., `web_fetch`, large `read` operations, or log outputs), hundreds or thousands of lines of raw text flood into the conversation context even when the model only needs a handful of lines matching a specific pattern. For example, fetching documentation on an API can consume 10,000–25,000 tokens of context, 98% of which is irrelevant boilerplate.
2. **Turn Latency Round-Trips**: Chaining operations (e.g. read file A, read file B, then search file C) requires sequential request-response inference round-trips with the model. Each turn adds latency and repeatedly re-transmits the growing conversation history.
3. **Platform Inconsistency**: While Unix users can theoretically chain commands via shell pipes (`curl ... | grep ...`), Windows environments lack native POSIX utilities (`grep`, `sed`, `awk`, `head`, `jq`), and pure `cmd.exe` fails on pipeline semantics and character encoding.

A native `script` tool that provides **batch tool execution with in-engine regex line filtering and context windows** enables zero-dependency, cross-platform tool pipelining that strips away intermediate noise before it ever touches the model context.

## Users and stakeholders

- **Agent Model**: Can batch tool calls into a single turn and apply regex filters directly to tool outputs, drastically reducing context bloat and avoiding context window exhaustion.
- **End Users**: Experience faster execution times, lower API token costs, and clean visual feedback in the terminal showing exact pipeline filters (`web_fetch ... | grep -C 2 "..."`).

## Goals

- **Single-Turn Batch Execution**: Execute an ordered sequence of tool calls in a single turn without LLM round-trips.
- **In-Engine Line Filtering**: Provide pure-Rust regex line filtering (`filter`) on any tool output so only matching lines and their surrounding context are returned to the model.
- **Surrounding Context Lines**: Support configurable context lines (`context`, defaulting to `2`, with `0` for match-only lines) mirroring `grep -C 2`, with automatic merging of overlapping match spans.
- **Clean UI Block Presentation**: Display pipeline executions in `BlockFormat` cards and live running tool widgets using familiar Unix pipe syntax (`tool args | grep -C 2 "pattern"`).
- **Calm Spinner Invariant**: Keep the activity spinner line (` ⠋ Working...`) completely untouched; all live status and step streaming are displayed in the running tool widget above the editor.
- **Safe Circuit-Breaking**: Automatically halt sequential execution if any step fails or is denied by permissions, returning accumulated outputs and the failing error without running subsequent steps.
- **Full Cross-Platform Support**: Work identically on macOS, Linux, and Windows (regardless of whether Git Bash, WSL, or POSIX tools are installed).

## Non-goals

- Implementing an arbitrary shell grammar or stream redirection engine (e.g. `stdin`/`stdout` byte pipe redirection between tools).
- Renaming existing tools (`rg`, `fd`, `bash`, `read`, `write`, `edit`, `web_search`, `web_fetch`).
- Creating a separate CLI process for filtering (all filtering is computed in-memory via the Rust `regex` crate).
- Altering the activity spinner text or animation.

## Current behavior

1. **Tool Invocation**: Tools are called individually one by one per turn (or in parallel tool call batches if the model emits parallel calls, but without dependency or output filtering).
2. **Output Transmission**: The full raw output of each tool (up to `output_max_bytes`) is serialized into a `ToolResult` message and appended to conversation history.
3. **MCP Script**: An `mcpScript` batch tool exists in `crates/rho-engine/src/mcp/gateway/mod.rs`, but only operates on MCP tools connected through `McpGateway` and does not support line filtering or built-in tools.
4. **Live Tool Display**: `RunningToolWidget` (`src/ui/interactive/layout/widget.rs`) renders the active tool above the editor, while the activity spinner line renders ` ⠋ Working...` below it.

## Desired behavior

1. **The `script` Tool**:
   - The model can invoke `script` with an array of `steps`:
     ```json
     {
       "steps": [
         {
           "tool": "web_fetch",
           "args": { "url": "https://docs.rs/tokio" },
           "filter": "RuntimeBuilder",
           "context": 2
         },
         {
           "tool": "read",
           "args": { "path": "src/main.rs" },
           "filter": "tokio::main"
         }
       ]
     }
     ```
2. **Filtering Semantics**:
   - If `filter` is omitted or null, the step output passes through in full (subject to standard tool truncation).
   - If `filter` is provided, the tool's text output is matched line-by-line against the regex.
   - `context` defaults to `2` if omitted. If set to `0`, only exact matching lines are returned.
   - For every match line $i$, lines in range $[\max(0, i - \text{context}), \min(N, i + \text{context})]$ are included.
   - Overlapping or adjacent ranges are merged into contiguous chunks.
   - Non-adjacent chunks are separated by `--\n`.
   - Matching lines are formatted with `:` (e.g. `42: let x = 1;`) and context lines with `-` (e.g. `41- fn test() {`).
   - If no lines match, returns `[No lines matched filter: "<regex>"]`.
3. **Execution Lifecycle**:
   - Steps run sequentially in array order.
   - Permissions are checked before executing each step. If a step is denied or errors, execution stops, partial step outputs are preserved, and the error is reported.
4. **UI Presentation**:
   - **Live Running Tool Widget**: Shows the active step with pipe notation, e.g.:
     `script (1/2) web_fetch https://docs.rs/tokio | grep -C 2 "RuntimeBuilder"`
     The tail output streams in real-time as chunks arrive.
   - **Transcript Block Card**: When completed, renders a unified card:
     ```text
      script (2 steps)

      1. web_fetch https://docs.rs/tokio | grep -C 2 "RuntimeBuilder"
         12- fn setup() {
         13:     let builder = RuntimeBuilder::new();
         14-     builder.build()

      2. read src/main.rs | grep "tokio::main"
         1: #[tokio::main]

      Took 480ms · 3 matches kept from 1,240 lines (98% reduction)
     ```
   - **Activity Spinner**: Stays strictly ` ⠋ Working...` with 0 dynamic text changes.

## Requirements

- **REQ-001**: Register `script` as a built-in tool across interactive and headless modes.
- **REQ-002**: `script` must accept `steps`: an array of `{ tool: string, args: object, filter?: string, context?: integer }`.
- **REQ-003**: `context` must default to `2` when omitted, and allow `0` for matching lines only.
- **REQ-004**: If `filter` is provided, compile as a case-insensitive smart-case regex, match against the tool output line-by-line, and format matching lines with `:` and context lines with `-`.
- **REQ-005**: Contiguous or overlapping match context spans must be merged into single blocks; separate non-contiguous blocks must be joined with `--\n`.
- **REQ-006**: If a step fails with an error or is denied by permissions, halt subsequent step execution, retain previously successful step outputs, and include the error message for the failed step.
- **REQ-007**: `script` steps must support both built-in tools (`read`, `write`, `edit`, `bash`, `fd`, `rg`, `web_search`, `web_fetch`) and registered MCP tools.
- **REQ-008**: The running tool widget must format active steps with Unix pipe syntax (`<tool> <summary> | grep -C <N> "<filter>"`).
- **REQ-009**: The activity spinner indicator line (`working_line_text`) must remain invariant (` ⠋ Working...`) and never carry script step names or arguments.
- **REQ-010**: All regex filtering and context span calculation must run in-process in pure Rust without spawning external shell processes.

## Invariants and security boundaries

- **Permission Parity**: Executing tool $T$ inside `script` must evaluate the exact same permission hooks, workspace boundaries, and mutation checks as calling $T$ directly.
- **Resource Limits**: Total accumulated script output must respect `output_max_bytes` to prevent memory exhaustion.
- **Regex Safety**: Regex compilation must be bounded (default size limit) to prevent ReDoS on malicious or degenerate regex inputs.
- **Spinner Purity**: The activity spinner line must never display step indices, tool names, or arguments.

## Definition of done

- Unit tests verify single-step execution, multi-step execution, filter matching, context expansion, overlap merging, non-match fallback, and error circuit-breaking.
- Running tool widget displays pipe syntax for filtered steps without UI jitter.
- Integration tests confirm both built-in and MCP tool execution within `script`.
- `cargo fmt --all -- --check`, `make clippy`, and `cargo test --workspace` pass cleanly.

## Risks and mitigations

- **Invalid Regex Syntax**: The model might emit an invalid regex (e.g. unclosed parenthesis).
  - *Mitigation:* Catch regex compilation error gracefully and report a clear tool error without crashing or breaking previous step outputs.
- **Permission Interrupts**: A step requiring interactive user confirmation could stall a batch.
  - *Mitigation:* Interactive prompts pause execution cleanly before the mutating step runs; if denied, previous read steps remain visible in the result.
- **Huge Pre-filter Output**: A command emits 100MB before filtering.
  - *Mitigation:* The tool's underlying output accumulator caps raw capture at `output_max_bytes` before regex filtering is applied.

## Out of scope

- Passing output streams from step $N$ directly as input arguments to step $N+1$ (data dependency graphing).
- Interactive editing of script steps mid-execution.
- User-defined script macros saved on disk.
