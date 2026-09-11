# Stream Batch Output Ordering Spec

## Status

Approved

## Problem

When the assistant produces its first token following a tool execution or thinking phase, `approval.rs` sends a prefix newline via `presenter.write_output("\n")` (`OutputEvent::Text`) to visually separate the preceding block from the response, followed immediately by `presenter.print_token(...)` (`OutputEvent::StreamText`) for the response's first streamed token.

Both events arrive synchronously in `PendingUiBatch`. Because `PendingUiBatch` separates outputs into two disconnected buckets (`text` and `stream_text`) and `LiveBatch::flush` processes `stream_text` before `text`, the order of terminal operations is inverted: the first token (e.g. `"Com"`) is written first, and the prefix newline (`"\n"`) is written second. When subsequent tokens (e.g. `"mitted in 78d510a:"`) arrive in subsequent batches, they are output on a new line, splitting the word across lines (e.g. `"Com\nmitted in 78d510a:"`).

## Users and stakeholders

- Interactive CLI / REPL users viewing live streaming assistant responses.
- Developers and agents reading transcript output in interactive sessions.

## Goals

- Preserve strict chronological ordering across all terminal output events (`OutputEvent::Text` and `OutputEvent::StreamText`) batched within a frame.
- Ensure prefix newlines separating tool blocks from streamed assistant text are written to the terminal before the first streamed text token.
- Coalesce adjacent output chunks of identical kinds so normal high-throughput token streaming remains a single merged write per frame.

## Non-goals

- Altering terminal resize replay behavior for `streamed_output` (`fbae4f2`).
- Changing non-interactive or line-mode output routing.
- Re-architecting tool execution or transcript persistence pipelines.

## Current behavior

`PendingUiBatch` maintains separate `text: String` and `stream_text: String` fields. When `drain()` runs, `PendingUiDrain` exposes separate `text` and `stream_text` strings. `LiveBatch::flush` (and test helpers) writes `drained.stream_text` first via `controller.write_stream_output()` and `drained.text` second via `controller.write_output()`. If a batch contains `OutputEvent::Text("\n")` followed by `OutputEvent::StreamText("Com")`, the output order is swapped to `"Com"` then `"\n"`.

## Desired behavior

`PendingUiBatch` maintains an ordered list of output events. Adjacent outputs of the same variant are coalesced into a single entry. When drained, outputs are written to the controller in exact arrival order. When a batch contains `OutputEvent::Text("\n")` followed by `OutputEvent::StreamText("Com")`, `"\n"` is written first and `"Com"` is written second. Subsequent tokens append directly after `"Com"`.

## Requirements

- REQ-001: `PendingUiBatch` must preserve the relative chronological arrival order of `OutputEvent::Text` and `OutputEvent::StreamText`.
- REQ-002: Consecutive outputs of the same kind pushed to `PendingUiBatch` must be coalesced in-place to avoid unnecessary allocations and multiple discrete writes.
- REQ-003: `PendingUiDrain` must expose outputs in their arrival order.
- REQ-004: `LiveBatch::flush` must iterate and execute drained outputs in arrival order, routing `OutputEvent::Text` to `write_output` and `OutputEvent::StreamText` to `write_stream_output`.
- REQ-005: Total pending output byte count tracking and flush decision barriers (newline, size limit) must remain identical in behavior.

## Invariants and security boundaries

- FIFO ordering: An output event emitted before another within the same thread or turn channel must never be written to the terminal after it.
- Secret redaction boundaries are unchanged (redaction happens prior to presenter calls in `approval.rs`).
- Zero-copy / low-allocation fast path for streaming tokens: adjacent `StreamText` tokens must append to the existing buffer without creating new vector elements.

## Definition of done

- Unit tests verify that `PendingUiBatch` preserves order across interleaved `Text` and `StreamText` events while coalescing consecutive matching variants.
- Controller / screen simulation tests demonstrate that a prefix newline preceding a streamed word does not split the word across lines.
- All workspace tests pass (`cargo test --workspace`).
- Clippy checks pass with zero warnings (`make clippy`).
- Formatting checks pass (`cargo fmt --all -- --check`).

## Acceptance criteria

- AC-001: Given `PendingUiBatch`, when `OutputEvent::Text("\n")` is pushed followed by `OutputEvent::StreamText("Com")`, `drain()` yields `outputs` where `Text("\n")` precedes `StreamText("Com")`.
- AC-002: Given `PendingUiBatch`, when multiple consecutive `OutputEvent::StreamText` chunks are pushed, `drain()` yields exactly one `StreamText` entry containing the concatenated text.
- AC-003: Given `TerminalController` and `LiveBatch`, when `OutputEvent::Text("\n")` followed by `OutputEvent::StreamText("Com")` and later `OutputEvent::StreamText("mitted")` are processed, the rendered screen displays `"\nCommitted"` without a newline between `"Com"` and `"mitted"`.

## Edge cases

- Empty strings pushed as `OutputEvent::Text("")` or `StreamText("")` must not create empty vector entries or corrupt cursor positions.
- Mixed interleaving: `Text`, `StreamText`, `Text`, `StreamText` within a single batch must execute as 4 ordered writes.
- Flush barriers: a newline inside either `Text` or `StreamText` must trigger `BatchDecision::Flush(FlushBarrier::Newline)`.
- Batch size threshold: the combined length of all batched outputs must be checked against `max_text_bytes`.

## Constraints

- Rust workspace conventions: no Clippy suppressions, shallow directory structure, files under ~300-400 lines where practical.
- Terminal performance: streaming hot path must not regress to per-token allocations.

## Risks and mitigations

- Risk: Existing tests relying on `drained.text` or `drained.stream_text` fail to compile. Mitigation: Update the small number of call sites in tests to iterate `drained.outputs` or provide convenience helpers where appropriate.

## References

- Commit `fbae4f2107e6203c824cc9cf47cc675126bf09f6` (`fix: preserve streamed output on terminal resize`).
- `src/ui/interactive/events/batch.rs`
- `src/repl/live/batch.rs`
- `crates/rho-engine/src/engine/runner/sink/approval.rs`
