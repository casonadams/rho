# Release readiness

This checklist is the release gate for the Rig 0.42 agent-core migration. Run it
without API keys, OAuth tokens, provider network access, or paid requests.

## Rollback boundary

Create an annotated release tag at the verified Slice 8 commit before
distribution. Rollback means reverting the application to that known commit or
to the pre-migration release; it does not mean converting session data.

Version-2 session files are intentionally unreadable by pre-migration builds.
After rolling back to a build without v2 support, start a fresh session and
leave existing JSONL, audit, checkpoint, and compaction files untouched. Do not
rewrite OAuth tokens into the API-key store. Provider-specific Rig token
directories may remain on disk for a later compatible build.

Stop release or rollback if any tool call/result pair is incomplete, a
checkpoint is appended as successful history before continuation succeeds, a
credential sentinel appears on a protected surface, an unknown provider reaches
network code, or content telemetry is enabled.

## Automated offline checks

From a clean checkout:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

The suite must exercise these structural behaviors with Rig test models,
temporary directories, and fake authorization boundaries:

- every API-key provider model constructs offline and maps to its own Rig
  identity;
- `openai`, `chatgpt`, and `copilot` remain distinct;
- mocked ChatGPT and Copilot login dispatch selects the requested OAuth
  provider;
- noninteractive OAuth reload does not start device authorization;
- denied write, edit, and mutating Bash calls have no side effects;
- sequential native tool calls retain complete call/result correlation;
- two-turn continuation and v2 `--resume` provide canonical history once;
- cancellation leaves no partial canonical assistant message or dangling tool
  call;
- approval selectors require Enter and Deny accepts optional blank feedback
  without executing the operation;
- model-call budget exhaustion sends no extra request;
- complete budget history becomes a separate checkpoint, survives reopen, and is
  promoted exactly once only after a successful continuation;
- a failed checkpoint continuation leaves the checkpoint available;
- Slice 6 core and session evaluation reports remain deterministic;
- Slice 7 windowing reduces long-session model-visible bytes while durable
  history remains valid and resumable;
- compaction remains bounded and restart-safe;
- the third identical consecutive tool call is blocked before execution;
- usage-unavailable reporting does not invent token counts;
- content telemetry remains disabled; and
- credential sentinels are absent from canonical messages, checkpoints,
  compaction artifacts, audits, metrics, errors, debug text, and rendered
  output.

## Static inspection

```sh
rg -n "chat/completions|eventsource|extract_tool_calls|OpenAiToolCallAccumulator" src
rg -n "#!\[(allow|expect)|#\[(allow|expect)\(clippy" src
cargo tree -d
```

The first two searches should return no obsolete custom provider HTTP/SSE or
fallback parser and no lint suppressions. Review direct dependencies against
source usage; duplicate transitive dependencies alone are not grounds for
unrelated upgrades.

Inspect representative temporary v2 fixtures produced by tests. They must have a
version header, ordered canonical and audit records, complete tool pairs,
terminal cancellation records where applicable, and no secret sentinel. A
pending run checkpoint must remain a separate record from canonical history.

## Credential-free runtime smoke

Use only deterministic tests for release automation. Do not perform a live OAuth
flow or provider completion as part of this checklist. Optional live smoke
testing requires an operator's explicit authorization and must not capture
tokens, prompts, or model output.

Before publishing, compare `rho --help`, interactive `/help`, and
[the README](../README.md). Confirm provider names, auth modes, mutation
approvals, provider-default output limits, finite model-call budgets, v2 resume
behavior, budget checkpoints, and the 24-message/8192-byte context defaults
agree.

## Pi-style interactive TTY validation

Validated commit `7a14111` on 2026-08-27 without credentials or provider network
access. Interactive runs used a deterministic local Ollama-compatible mock; no
live user prompts, model output, or credentials were recorded.

| Terminal path                                         | Result        | Coverage                                                                                                                                     |
| ----------------------------------------------------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Pi harness: Alacritty through tmux (`xterm-256color`) | Pass          | Normal-screen output, native scrollback, streaming, steering/follow-up queueing, cancellation, and clean exit.                               |
| Terminal.app through tmux (`xterm-256color`)          | Pass          | Narrow/wide resize, long wide-Unicode draft, raw-LF `Ctrl+J`, streaming, both queue modes, native scrollback, and terminal-mode restoration. |
| iTerm2                                                | Not available | `/Applications/iTerm.app` was not installed; rerun this matrix when iTerm2 is available.                                                     |

The available-terminal matrix covered these release conditions:

- The editor grew for explicit newlines, wrapping, and wide Unicode while both
  `─` dividers and the footer remained intact at 38x16 and 100x30.
- Modified Enter sequences and raw-LF `Ctrl+J` were exercised without unintended
  submission. Legacy `Alt+Enter` queued follow-up intent; steering and follow-up
  entries displayed a count and ran FIFO after the active response.
- A 120-chunk response streamed as continuous wrapped prose in a 40x12 viewport.
  Completed output remained above the live region in normal terminal scrollback.
- Bash approval, turn-limit confirmation, and agent-question modals rendered in
  the live region. Escape canceled each modal, restored editor operation, and
  did not execute the denied command.
- Both `Ctrl+C` and Escape canceled active work. `Ctrl+D` exited an empty idle
  editor and restored the pre-run terminal mode.
- A forced PTY input disconnect returned an input error and restored canonical
  input, echo, signal handling, and control characters before exit.
  Deterministic backend-failure tests additionally verify cursor visibility and
  raw-mode cleanup when controller construction or output fails.
- Existing tests verified that non-TTY and one-shot paths do not enter the live
  controller. Direct plugin or subprocess stdout remains outside the controller
  boundary as documented in the README.

A clean snapshot at the validated commit passed:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The test suite reported 306 passed and 0 failed. Repeat the terminal matrix
after changes to controller cursor accounting, input decoding, live-region
layout, or modal coordination.

### Decoupled-input streaming stress validation

Slice 5 uses a named terminal-input thread and a 16 ms output frame. Run the
stress test without credentials or provider access:

```sh
cargo test streaming_flood_preserves_output_and_applies_input_within_two_frames
cargo test repl::input_reader -- --test-threads=1
cargo test ui::interactive::controller -- --test-threads=1
```

The deterministic flood sends 10,000 six-byte fragments through the pending
batch while editor input is ready. It compares all 60,000 output bytes, records
the first visible input frame, and fails on loss, duplication, reordering, or
input latency above two frames. The controller test verifies that 1,000 adjacent
fragments produce one output write and one terminal flush. Input-reader tests
cover forwarding, acknowledged pause, no reads while paused, propagated errors,
receiver closure, and joined shutdown.

For each supported interactive terminal, use a credential-free deterministic
local provider mock that emits at least 10,000 small chunks. Keep the terminal
on its normal screen and perform this sequence:

1. Hold a key while moving the cursor through a multiline wide-Unicode draft.
   Submit once with Enter and once with Alt+Enter.
2. Resize repeatedly between approximately 40 and 100 columns while output is
   active. Open and cancel an approval or question modal.
3. Confirm the draft changes within 32 ms, streamed bytes remain ordered above
   the editor, and the terminal's native scrollback contains the beginning and
   end sentinels.
4. Compare `stty -g` before and after clean exit, active cancellation, forced
   input disconnect, and a slash command that temporarily owns the terminal.
   Confirm echo, canonical mode, signals, and cursor visibility are restored.

Record only terminal type, pass/fail, latency, byte count, scrollback outcome,
and cleanup outcome. Never record prompts, session output, environment values,
or credentials.

| Validation path                  | Result on 2026-08-27 | Input latency                                                      | Output                           | Cleanup                           |
| -------------------------------- | -------------------- | ------------------------------------------------------------------ | -------------------------------- | --------------------------------- |
| Deterministic in-process flood   | Pass                 | Frame 0, below two-frame limit                                     | 60,000/60,000 bytes exact        | Not applicable                    |
| Input/controller lifecycle tests | Pass                 | Immediate channel delivery                                         | 1,000 fragments, one flush       | Pause, error, drop, and join pass |
| Pi harness interactive terminal  | Not rerun            | Non-interactive coding session cannot measure key-to-paint latency | Native scrollback not observable | Automated cleanup passed          |
| Terminal.app                     | Not rerun            | Requires interactive operator                                      | Requires interactive operator    | Requires interactive operator     |
| iTerm2                           | Unavailable          | Application not installed                                          | Application not installed        | Application not installed         |

The prior manual matrix remains evidence for normal-screen behavior, but release
is blocked until the Pi harness and Terminal.app rows are rerun against the
Slice 5 commits.
