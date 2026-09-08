# Preserve In-Flight Stream Output Across Terminal Resizes

## Problem

During a live turn, rendered assistant tokens are written directly to the terminal but are not committed to the controller transcript until the turn completes. A terminal resize invokes a full redraw, which clears the terminal and redraws only committed transcript items and the live input region. The in-flight response disappears until the completed response is committed and another redraw occurs.

### Current flow

1. `TerminalRenderer::print_token()` appends raw tokens to `assistant_turn_buffer`, Markdown-renders the token, and emits `UiEvent::Output`.
2. `LiveBatch::flush()` sends the event to `TerminalController::write_output()`, which directly paints it to the terminal.
3. `TerminalRenderer::flush()` emits `TranscriptItem::AssistantText` only when the response finishes.
4. `TerminalController::refresh_size()` calls `full_redraw()` when transcript history exists.
5. `full_redraw()` clears the terminal and replays `TerminalController.transcript`, but has no representation of the uncommitted streamed output.

## Goal

A resize during an active assistant stream must preserve all received response content, permit subsequent chunks to continue normally, and leave exactly one committed response after completion.

## Non-Goals

- Re-render the entire response from raw Markdown on every streamed token.
- Change persisted transcript or session behavior.
- Change line-mode or non-interactive presentation.
- Add extra terminal redraws during normal token streaming.

## Requirements

- Previously received streamed assistant output remains visible after a resize.
- Subsequent chunks resume at the correct output location.
- The completed assistant response is shown exactly once after the final transcript item is committed.
- Cancellation, provider errors, and turn resets cannot leave stale stream output for later turns.
- Existing output batching (16 ms frame interval / 16 KiB pending output) and terminal write behavior remain unchanged.

## Design

### Make streamed assistant output explicit

Do not make all `TerminalController::write_output()` calls persistent. Introduce a stream-specific event/output kind so the controller can distinguish the active assistant response from arbitrary direct output:

```rust
enum OutputEvent {
    Text(String),
    StreamText(String),
}
```

`TerminalRenderer::print_token()` and final Markdown fragments emitted by `TerminalRenderer::flush()` use `StreamText`. Existing generic `Text` output remains unchanged.

### Retain pre-rendered stream chunks in the controller

Add a turn-local controller buffer:

```rust
streamed_output: String
```

When a `StreamText` batch is processed:

1. append the already-rendered terminal text to `streamed_output`;
2. paint it using the existing direct-output and `OutputTracker` machinery;
3. retain it until its assistant transcript item is committed or the active turn is reset.

The buffer contains terminal-ready output, including ANSI styling. It does not require an extra Markdown parse.

### Replay during a full redraw

Change `TerminalController::run_full_redraw()` to paint in this order:

```text
clear terminal
reset OutputTracker
repaint committed transcript
replay in-flight streamed output, if present
render live widget / editor / footer
restore cursor
flush
```

Replay must use the same newline normalization and `OutputTracker` updates as normal streamed output, including the existing handling of a final open output line before the live input layout is rendered.

### Commit and reset lifecycle

The controller should expose focused lifecycle methods, for example:

```rust
write_stream_output(&mut self, output: &str)
commit_streamed_output(&mut self)
clear_streamed_output(&mut self)
```

When `TranscriptItem::AssistantText` is accepted, it becomes the durable representation of the response and the controller clears the transient stream buffer. The buffer must remain available through final Markdown flushing and until the transcript item has been processed.

On cancellation, provider/turn error without an assistant transcript item, or defensive new-turn initialization, clear the transient buffer.

| Situation | Stream buffer behavior |
| --- | --- |
| Assistant token/chunk | Append and retain |
| Terminal resize | Replay and retain |
| Successful completion | Clear after `AssistantText` transcript commit |
| Cancellation after renderer flush | Clear after partial assistant transcript commit |
| Error without transcript | Clear during active-turn reset |
| New turn | Assert/reset empty state defensively |

## Performance

### Normal streaming

The existing hot path remains:

```text
token -> MarkdownRenderer -> PendingUiBatch -> terminal write
```

The only added steady-state work is appending the pre-rendered chunk to a string. There is no second Markdown render, extra terminal write, or additional redraw per token. Existing batching remains unchanged.

### Resize

A resize already requires a repaint because wrapping may change. The extra resize-only work is proportional to the active response length:

```text
O(committed transcript + in-flight rendered response)
```

This is necessary to reconstruct the screen and does not affect normal streaming throughput.

### Memory

`TerminalRenderer.assistant_turn_buffer` already retains the raw response until completion. The controller additionally retains its rendered terminal representation only for the active turn. It is released when the response is committed or discarded.

## Implementation Plan

1. Update `src/ui/interactive/events/types.rs` to distinguish stream output from generic output.
2. Update `src/ui/interactive/events/batch.rs` and `src/repl/live/batch.rs` to preserve current batching while routing stream output to controller stream methods.
3. Update `src/ui/render/renderer/mod.rs` so `print_token()` and the final Markdown flush use the stream-specific path.
4. Add `streamed_output` and focused write/commit/reset methods to `src/ui/interactive/controller/mod.rs` or an appropriate controller module.
5. Extend `src/ui/interactive/controller/transcript.rs::run_full_redraw()` to replay the in-flight buffer after committed history and before the live region.
6. Clear transient output after `TranscriptItem::AssistantText` is processed; clear it on cancellation/error/reset paths as needed.
7. Add regression coverage using `FakeTerminal`:
   - stream output then resize replays existing content;
   - output can continue after a resize;
   - committed assistant content is not duplicated on a later resize;
   - cancel/reset clears stale partial stream output;
   - current high-volume stream batching remains one terminal write/flush per drained batch.
8. Validate with:
   - `cargo fmt --all -- --check`
   - focused controller/live-batch tests
   - `make clippy`
   - `cargo test --workspace`

## Acceptance Criteria

Manual validation:

1. Start a response that streams for several seconds.
2. Resize narrower and wider while it is streaming.
3. Content received before each resize stays visible.
4. New content follows the preserved response correctly.
5. Complete or cancel the turn.
6. Resize again.
7. The completed or partial response appears exactly once, with no stale output or duplication.
