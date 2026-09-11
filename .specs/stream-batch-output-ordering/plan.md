# Stream Batch Output Ordering Plan

## Research

- In `src/ui/interactive/events/batch.rs`:
  - `PendingUiBatch` currently stores `text: String` and `stream_text: String`.
  - `push` routes `OutputEvent::Text` to `self.text` and `OutputEvent::StreamText` to `self.stream_text`.
  - `PendingUiDrain` exposes `text: String` and `stream_text: String`.
  - `is_empty` checks `self.text.is_empty() && self.stream_text.is_empty()`.
  - `flush_barrier_for_text` checks `self.text.len() + self.stream_text.len() >= self.max_text_bytes`.
- In `src/repl/live/batch.rs`:
  - `LiveBatch::flush` flushes `drained.stream_text` before `drained.transcript_items` and before `drained.text`.
- In `src/ui/interactive/controller/tests/screen_sim.rs`:
  - `drive_renderer_to_controller` replicates the order from `LiveBatch::flush`.
- In `src/ui/interactive/controller/tests/output.rs`:
  - `controller.write_output(&pending.drain().text)` accesses `.text`.
- In `src/ui/interactive/events/tests/batch.rs`:
  - Tests check `.text` and `.stream_text`.

## Reuse

- Reuse existing `OutputEvent` enum (`src/ui/interactive/events/types.rs`), which already has `Text(String)` and `StreamText(String)` variants with `Clone, PartialEq, Eq`.
- Reuse `TerminalController::write_output` and `TerminalController::write_stream_output`.
- Reuse `FakeTerminal` and `ScreenBackend` test harness infrastructure.

## Invariants and security boundaries

- Ordering invariant: Output events are flushed strictly in the order received.
- Allocation invariant: Consecutive tokens of the same variant append to the existing buffer instead of allocating new vector entries.
- Flush barriers invariant: Newline and max text bytes barriers trigger flush identically to current behavior.

## Vertical Slices

### Slice 1: Ordered Output Batching in `PendingUiBatch` (Score: 2)

**Outcome:** `PendingUiBatch` stores an ordered `outputs: Vec<OutputEvent>` with in-place coalescing of identical adjacent variants. `PendingUiDrain` exposes `outputs: Vec<OutputEvent>`.

- **Task 1.1:** Add `outputs: Vec<OutputEvent>` and `total_output_bytes: usize` to `PendingUiBatch`.
  - Files: `src/ui/interactive/events/batch.rs`
  - In `push`, call `push_output` which coalesces into `outputs.last_mut()` if variant matches or appends a new variant.
  - In `drain`, transfer `outputs: std::mem::take(&mut self.outputs)` and reset `self.total_output_bytes = 0`.
- **Task 1.2:** Update `PendingUiDrain` to contain `pub outputs: Vec<OutputEvent>`. Provide a helper `pub fn text(&self) -> String` if needed for backwards compatibility or tests, or update callers directly.
  - Files: `src/ui/interactive/events/batch.rs`
- **Task 1.3:** Update unit tests in `src/ui/interactive/events/tests/batch.rs` to verify chronological preservation and adjacent coalescing.
  - Files: `src/ui/interactive/events/tests/batch.rs`
  - Verifies: AC-001, AC-002

### Slice 2: Chronological Flushing in `LiveBatch` and Regression Tests (Score: 3)

**Outcome:** `LiveBatch::flush` writes outputs in exact arrival order. The word-splitting bug when a prefix newline precedes streamed tokens is fully resolved.

- **Task 2.1:** Update `LiveBatch::flush` in `src/repl/live/batch.rs` to iterate over `drained.outputs` and dispatch each variant to `controller.write_output` or `controller.write_stream_output`.
  - Files: `src/repl/live/batch.rs`
- **Task 2.2:** Update `drive_renderer_to_controller` in `src/ui/interactive/controller/tests/screen_sim.rs` and any caller in `src/ui/interactive/controller/tests/output.rs`.
  - Files: `src/ui/interactive/controller/tests/screen_sim.rs`, `src/ui/interactive/controller/tests/output.rs`
- **Task 2.3:** Add an observable regression test in `src/ui/interactive/controller/tests/screen_sim.rs` simulating a tool finish followed immediately by prefix blank line and streamed tokens (`"Com"` then `"mitted in 78d510a:"`), asserting that `"Committed in 78d510a:"` is rendered on a single line.
  - Files: `src/ui/interactive/controller/tests/screen_sim.rs`
  - Verifies: AC-003

## Quality Gates

- `cargo fmt --all -- --check`
- `make clippy`
- `cargo test --workspace`

## Definition of Done

- All tasks in Slices 1 and 2 implemented.
- Defect detection regression test added and verified.
- Full workspace tests, linter, and formatter pass cleanly with zero warnings or suppressions.
