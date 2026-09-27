# Context Window and Token Optimization Spec

## Status

Implemented (Commit: `5815f3e`)

## Problem

In long-running and multi-turn interactive sessions, `rho`'s token consumption, prompt cache hit rate, and API latency encounter critical inefficiencies compared to state-of-the-art agents like `oh-my-pi` and `OpenCode`:

1. **Prompt Cache Invalidation via Naive Historical Pruning**:
   Currently, `crates/rho-engine/src/engine/runner/turn/prepare.rs` executes `prune_historical_tool_outputs` immediately after a single turn (`volatile_turns = 1`). Whenever a tool call in Turn $N-1$ produced verbose output (>15 lines), Turn $N$ mutates that turn in-place in memory before sending the history to the LLM. Because the prefix tokens of Turn $N-1$ changed, Anthropic's and OpenAI's KV prompt caches are invalidated. Instead of receiving an 80–90% prompt cache read discount and near-instant TTFT, every multi-turn interaction triggers expensive cache writes.
2. **Context Bloat from Superseded Reads**:
   When files are read repeatedly or modified across turns, earlier `read` results and stubs remain in the context, consuming context window capacity without informational value.
3. **Lossy Tool Truncation**:
   When bash commands, file searches, or grep commands produce outputs exceeding line or byte limits (e.g., 2,000 lines or 50KB), the current truncate implementation clips the stream irreversibly. The model and user lose the ability to inspect truncated segments without re-running the entire command.
4. **Synchronous Turn Pause During Compaction**:
   When context utilization hits the compaction threshold, compaction runs synchronously mid-turn or between turns, causing a 15–30 second interactive pause for the user.
5. **Summarizer Context Blowout on Media Attachments**:
   If user messages contain base64 image attachments or large media blocks, feeding the raw history into the compaction summarizer can cause the compaction request itself to overflow the model's context window.

## Users and stakeholders

- **Interactive CLI Users**: Experiencing unnecessary API costs, high time-to-first-token (TTFT) on multi-turn sessions, lost data from truncated commands, and interactive blocking during compaction.
- **Agent Reasoning Engine (`rho-engine`)**: Needs maximum effective context window capacity and high cache hit rates without losing access to previous tool execution details.

## Goals

1. **Cache-Aware Tool Pruning**: Protect recently transmitted prompt prefixes from mutation so provider KV prompt caches (Anthropic, OpenAI) remain intact across turns (default: 30k–40k tokens protected, 20k token minimum savings floor).
2. **Protected Tool Exclusion**: Guarantee that high-value reference tools like `skill` results are never pruned.
3. **Superseded Tool Result Elision**: Identify file reads that have been made obsolete by newer reads or file edits, and compress or elide them during compaction passes.
4. **Spillover Artifact Storage via `rig::memory::DemotionHook`**: When tool outputs exceed byte/line thresholds, persist the complete, lossless output to disk in `.rho/artifacts/{id}.log` via a demotion hook, and return a structured reference URI so the model or user can read specific ranges.
5. **Speculative Background Compaction**: Trigger background pre-compaction on a snapshot when context crosses an 85% lead threshold, so when the 100% threshold is reached, the summary is ready with near-zero latency.
6. **Media Stripping on Compaction**: Strip raw image/binary media buffers from messages passed to the summarizer, substituting lightweight media metadata to guarantee the summarizer request never overflows.

## Non-goals

- Implementing bitmap font rendering (`snapcompact` / PNG generation), which introduces heavy font and image dependencies outside rho's lean CLI scope.
- Hard model changes or automatic switching of active models without explicit user consent.
- Changing JSON schema definitions in a way that breaks strict tool validation on existing upstream providers.

## Current behavior

1. In `prepare.rs`, `prune_historical_tool_outputs(history, 1, 15)` runs before every turn request. If turn 1 ran a 100-line `cargo test`, turn 1's output is rewritten to a 1-line stub when preparing turn 2. This busts the prefix prompt cache for turn 2.
2. If file `foo.rs` is read in Turn 1, modified in Turn 2, and read again in Turn 3, Turn 1's read result still remains in conversation history as a stub or verbatim content.
3. In `tools/truncate.rs`, when a tool output exceeds 2,000 lines or 50KB, `truncate_output` simply truncates the string with `... [truncated]`. The truncated output is lost permanently.
4. In `auto_compact.rs`, auto-compaction is checked and executed synchronously inside the turn loop via `AutoCompactHook`. When compaction triggers, the turn blocks while the LLM generates the summary.
5. In `llm.rs`, summarization passes all message content, including media blocks, directly to the summarizer model.

## Desired behavior

1. **Cache-Aware Pruning**:
   - Pruning protects the most recent $N$ tool tokens (default: 30,000 tokens) from historical pruning.
   - Pruning requires a minimum estimated savings threshold (default: 20,000 tokens) before mutating history.
   - Outputs below 50 tokens (`MIN_PRUNE_TOKENS`) are never pruned to avoid churning the cache for negligible gains.
   - Tools marked as protected (e.g. `skill`) are excluded from pruning.
2. **Superseded Result Tracking**:
   - Tool execution tracks file paths accessed by `read`, `write`, and `edit`.
   - When a newer read or write occurs for path $P$, older read results for path $P$ are marked superseded and replaced with `[Superseded by later read/edit of <path>]` during compaction passes.
3. **Lossless Artifact Spilling**:
   - When a tool output exceeds the maximum byte limit (default 50KB), the full content is written to `.rho/artifacts/<uuid>.log` via an `ArtifactDemotionHook` implementing `rig::memory::DemotionHook`.
   - The tool result presents the initial head/tail excerpt followed by:
     `[Output truncated. Full content (145KB, 3,210 lines) saved to .rho/artifacts/<uuid>.log. Use 'read' or 'bash' with grep on this file if needed.]`
4. **Speculative Pre-Compaction**:
   - When estimated context tokens reach 85% of `compaction_threshold` (`[threshold - lead_tokens, threshold]`), a background task begins speculative summarization on a history snapshot.
   - If the session reaches the threshold without branch divergences, the speculative summary is adopted immediately, reducing compaction latency to milliseconds.
5. **Media Stripping for Summarizer**:
   - During summarizer message prep, media parts (base64 image blocks) are replaced with `[Image: <mime> (<width>x<height>)]` stubs before dispatching to the compaction prompt.

## Requirements

- **REQ-001**: `prune_historical_tool_outputs` MUST respect a `protect_recent_tokens` buffer (default: 30,000 tokens) and MUST NOT prune tool results falling inside this recent window.
- **REQ-002**: `prune_historical_tool_outputs` MUST require at least `prune_minimum_tokens` (default: 20,000 tokens) in estimated savings before mutating historical messages.
- **REQ-003**: Tool results whose raw length is below `MIN_PRUNE_TOKENS` (50 tokens / ~200 characters) MUST NOT be pruned into stubs.
- **REQ-004**: Results from protected tools (`skill`) MUST be exempt from pruning.
- **REQ-005**: Tool executions MUST record touched file paths. When path $P$ is subsequently read or modified, older completed `read` tool results for path $P$ MUST be identified as superseded during compaction.
- **REQ-006**: When any tool output exceeds `output_max_bytes` or `output_max_lines`, the full original output MUST be persisted to `.rho/artifacts/{id}.log` via `ArtifactDemotionHook`, and the truncation notice MUST include the artifact file path and byte/line counts.
- **REQ-007**: Artifact log creation MUST fail gracefully if disk writes fail, preserving in-memory truncated output without crashing the tool execution.
- **REQ-008**: Artifact directories (`.rho/artifacts/`) MUST be automatically created and registered in session cleanup / retention policies.
- **REQ-009**: `AutoCompactHook` MUST support an optional speculative pre-compaction state that triggers summarization in a detached `tokio::spawn` task when context utilization exceeds `speculative_threshold_ratio` (default 0.85).
- **REQ-010**: Image and binary media parts MUST be stripped and replaced with text metadata stubs before submitting history to the compaction LLM.
- **REQ-011**: All new domain logic MUST maintain Campbell cognitive complexity $\le 15$ and CRAP score $\le 30$.

## Invariants and security boundaries

- **Zero Data Loss**: Truncated tool outputs must be preserved losslessly in `.rho/artifacts/` whenever filesystem permissions permit.
- **Privacy and Secrets**: Artifact logs must respect the user's workspace permissions and must not be exposed outside `.rho/`.
- **Pure Domain Rules**: Superseded identification and pruning thresholds must remain deterministic and testable without active network connections.
- **Prompt Cache Stability**: Suffixes and prefixes already sent to the LLM must not be mutated between back-to-back turns unless the cache is explicitly acknowledged as cold or being reset by a compaction cut.

## Definition of done

1. Unit tests in `rho-engine` proving:
   - Tool outputs within the `protect_recent_tokens` window remain unpruned across turn boundaries.
   - Savings below `prune_minimum_tokens` aborts historical mutation to preserve cache.
   - Tool outputs below `MIN_PRUNE_TOKENS` are skipped by pruning.
   - `skill` tool outputs are never pruned.
   - Superseded read results are detected and replaced during compaction.
   - Truncated tool outputs generate on-disk artifacts and accurate pointer messages.
   - Speculative compaction produces valid summary nodes when context limits are reached.
   - Media parts are stripped during summarization calls.
2. Full validation suite passes: `make complexity`, `make crap`, `make test`, `make clippy`, `make fmt`.

## Acceptance criteria

- **AC-001**: Given a conversation where Turn 1 produces an 80-line tool output (500 tokens), when Turn 2 begins with `protect_recent_tokens = 30_000`, the Turn 1 tool output remains 100% verbatim in the request payload so the prompt cache hits.
- **AC-002**: Given a session with only 5,000 prunable tokens, pruning is skipped because it does not meet `prune_minimum_tokens = 20_000`.
- **AC-003**: Given a 30-token tool output, when pruning runs, the output is not replaced with a 15-token stub.
- **AC-004**: Given Turn 1 reads `src/main.rs`, and Turn 3 edits `src/main.rs`, during subsequent compaction the Turn 1 read result is marked superseded.
- **AC-005**: Given a bash command emitting 200KB of logs, when executed, the tool returns a 50KB excerpt referencing `.rho/artifacts/<id>.log`, and the complete 200KB log exists on disk.
- **AC-006**: Given context size exceeding 85% of compaction threshold, a background task spawns to pre-compute the summary snapshot.
- **AC-007**: Given a history containing a base64 image message, during compaction the image data is replaced by a metadata stub before dispatching to the summarizer.

## Edge cases

- Truncation on systems where disk write fails or `.rho` directory is read-only: falls back to in-memory truncation without error.
- Non-UTF8 or binary tool output: correctly encoded or sanitized before artifact logging and previewing.
- Rapid branch switching during speculative compaction: if the branch diverged before the speculative compaction finished, the speculative result is cleanly discarded.

## Constraints

- Rust 2024 edition, zero Clippy warnings (`-D warnings`).
- Campbell cognitive complexity $\le 15$ per function.
- CRAP score $\le 30$ per function.

## Risks and mitigations

- **Risk**: Excessive artifact files accumulating in `.rho/artifacts/`.
  - **Mitigation**: Implement LRU or age-based artifact retention during session startup/cleanup.
- **Risk**: Increased memory usage from keeping 30k tokens unpruned.
  - **Mitigation**: 30,000 tokens of text is ~120KB of RAM, well within CLI memory budgets, and matches typical 1-hour provider prompt cache lifetimes.

## References

- `crates/rho-engine/src/engine/runner/turn/prune.rs`
- `crates/rho-engine/src/engine/runner/turn/prepare.rs`
- `crates/rho-engine/src/tools/truncate.rs`
- `crates/rho-engine/src/engine/runner/turn/auto_compact.rs`
- `../oh-my-pi/docs/compaction.md`
- `../oh-my-pi/packages/agent/src/compaction/pruning.ts`
- `https://opencode.ai/v2/docs/compaction`
- `https://deepwiki.com/sst/opencode/2.4-context-management-and-compaction`
