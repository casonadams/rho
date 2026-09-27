# Context Window and Token Optimization Plan

## Status: Completed (Implemented in commit `5815f3e`)

## 1. Research

- **Target Architecture**:
  - `crates/rho-engine/src/engine/runner/turn/prune.rs`: Currently performs in-place historical pruning on all turns older than `volatile_turns = 1` if line counts exceed 15 lines. This invalidates prefix prompt caches.
  - `crates/rho-engine/src/tools/truncate.rs`: Currently performs hard truncation at byte and line limits (`DEFAULT_OUTPUT_MAX_BYTES = 50_000`, `DEFAULT_OUTPUT_MAX_LINES = 2000`), discarding truncated lines completely.
  - `crates/rho-engine/src/engine/runner/turn/auto_compact.rs`: Evaluates token thresholds on each turn/step and runs synchronous summarization when context size exceeds `context_window - reserve_tokens`.
  - `crates/rho-engine/src/engine/compactor/llm.rs`: Dispatches summarization prompts; needs media stripping to prevent summarizer overflow on base64 images.
- **Upstream & Peer Agent Research**:
  - `../oh-my-pi`: Uses `protectRecentTokens` (40k tokens), `MIN_PRUNE_TOKENS` (50 tokens), superseded file read elision, on-disk artifact spilling, and background speculative compaction in the lead band `[threshold - lead, threshold)`.
  - `OpenCode` (`sst/opencode`): Uses `PRUNE_PROTECT` (40k tokens), `PRUNE_MINIMUM` (20k tokens savings floor), protected tool exclusions (`skill`), media attachment stripping before compaction, and structured summary sections (`Objective`, `Next Move`, `Completed & Active Work`, `Blockers`, `Relevant Files`).
  - `rig` / `rig-memory` v0.42: Provides `rig::memory::DemotionHook` (called when messages or outputs are evicted), `rig::extractor::ExtractorBuilder` for JSON-schema constrained extraction, `rig_memory::TokenCounter`, and `rig_memory::TemplateCompactor`.

## 2. Reuse

- `rig::memory::DemotionHook`: Implement `ArtifactDemotionHook` to handle lossless on-disk persistence of evicted/pruned tool outputs without coupling tool runners directly to file I/O.
- `crates/rho-harness-core/src/tokens/memo.rs`: Reuse `MessageTokenCache` for token footprint calculations.
- `crates/rho-harness-core/src/session/compaction/files.rs`: Reuse `extract_file_ops` for tracking file paths accessed across turns.
- `crates/rho-harness-core/src/tokens/mod.rs`: Reuse `BpeTokenCounter` (implementing `rig_memory::TokenCounter`).

## 3. Invariants and security boundaries

- **Zero Information Loss**: Spilled tool outputs must be written losslessly to disk in `.rho/artifacts/` whenever filesystem permissions permit.
- **Cache Prefix Invariance**: Historical messages within the protected recent window or active cache prefix must not be mutated, guaranteeing stable KV cache prefixes across turns.
- **Fail-Safe Degradation**: If artifact writing fails (e.g. disk full, permission denied), fallback to in-memory truncation without failing the tool execution.
- **Complexity and Architecture**: Pure domain rules remain in domain modules (`prune.rs`, `truncate.rs`), separate from CLI rendering or HTTP adapters. Every function must have cognitive complexity $\le 15$ and CRAP $\le 30$.

## 4. Quality gates

- `make complexity`: Ensures all modified functions maintain Campbell cognitive complexity $\le 15$.
- `make crap`: Ensures all modified functions maintain CRAP score $\le 30$.
- `make test`: All workspace unit and integration tests pass.
- `make clippy`: Zero Clippy warnings (`-D warnings`).
- `make fmt`: Formatting compliant.

## 5. Definition of done

- `prune_historical_tool_outputs` supports:
  - `protect_recent_tokens` (default: 30,000 tokens),
  - `prune_minimum_tokens` (default: 20,000 tokens savings floor),
  - `min_prune_tokens` (default: 50 tokens floor),
  - Exemption for protected tools (`skill`).
- Superseded file reads are detected and elided during compaction passes.
- Media parts are stripped and replaced with text metadata stubs before summarization.
- Truncated tool outputs are persisted to `.rho/artifacts/` via `ArtifactDemotionHook` with user/model reference notes.
- Speculative compaction asynchronously prepares summaries in the lead band before hard thresholds are crossed.
- Unit and integration tests cover all new behaviors and edge cases.

## 6. Assumptions

- Modern LLM provider prompt caches (Anthropic, OpenAI) persist for 5 minutes to 1 hour; preserving the last 30,000 tokens of tool output ensures multi-turn sessions hit the warm cache prefix on subsequent turns.
- The `.rho/` directory in the current working directory or user home is writable during interactive sessions.

## 7. Risks (with mitigation)

- **Risk**: Uncontrolled disk growth in `.rho/artifacts/`.
  - **Mitigation**: Implement an LRU or age-based artifact retention policy that prunes artifacts older than 7 days or capping directory size at 100MB.
- **Risk**: Branch switching rendering speculative compaction invalid.
  - **Mitigation**: Speculative compaction tasks tag the generation with the source node/turn ID; if the active session branch diverges before completion, the background task result is safely dropped.

## 8. Dependencies

- No new external crates required. All needed primitives (`tokio`, `uuid`, `serde`, `std::fs`, `rig`, `rig-memory`) are already in workspace dependencies.

## 9. Decisions

- **Decision**: Set `protect_recent_tokens` to 30,000 tokens and `prune_minimum_tokens` to 20,000 tokens (conforming to OpenCode and oh-my-pi benchmarks).
  - **Tradeoff**: Retains more tokens in memory for recent turns, but saves up to 90% on API billing and eliminates prompt-cache thrashing.
- **Decision**: Implement `ArtifactDemotionHook` adhering to `rig::memory::DemotionHook`.
  - **Tradeoff**: Standardizes eviction and spilling across compaction and pruning using existing rig abstractions.

## 10. Out of scope

- Rendering bitmap images for discarded history (`snapcompact`).
- Dynamic switching of model tiers during mid-turn overflow.

---

### Slice 1: Cache-Aware Tool Pruning, Exclusions & Superseded Read Elision

#### Goal
Prevent prompt-cache churn by preserving recent tool output tokens, exempting protected tools (`skill`), enforcing a 20k savings floor, and eliding redundant historical file reads when newer reads/edits have superseded them.

#### Acceptance criteria
- Tool results within the protected token window (30k tokens) are never pruned into stubs.
- Pruning does not mutate history unless total estimated savings exceed 20k tokens.
- Tool results under 50 tokens and tools named `skill` are never pruned.
- When file $P$ is read in Turn 1 and subsequently edited or re-read in Turn 3, Turn 1's read result is marked superseded and replaced with a concise superseded notice during compaction.

#### Task 1.1: Add cache-protection window, minimum savings floor, and tool exclusions to pruning [2]
**Do:** Update `prune_historical_tool_outputs` in `crates/rho-engine/src/engine/runner/turn/prune.rs` to accept `protect_recent_tokens: usize`, `prune_minimum_tokens: usize`, and `min_prune_tokens: usize`. Skip pruning any output within the protected window, below `min_prune_tokens`, or with tool name `skill`. If total potential savings $< prune\_minimum\_tokens$, abort pruning entirely to preserve cache prefix.
**Tests:** Unit tests verifying:
1. Tool outputs within the protected token window remain untouched.
2. If total savings $< 20,000$ tokens, history is left unchanged.
3. Outputs $< 50$ tokens and `skill` outputs are preserved.
**Verify:** `cargo test -p rho-engine engine::runner::turn::prune` -- passes.

#### Task 1.2: Implement superseded file read identification [3]
**Do:** In `crates/rho-engine/src/engine/runner/turn/prune.rs` (or `superseded.rs`), track file paths touched by `read`, `write`, and `edit`. For multiple reads of the same path across turns, identify all but the latest read as superseded and replace with `[Superseded by later read of <path>]` during compaction.
**Tests:** Unit tests verifying multiple reads of the same file mark earlier reads as superseded, while reads of different files remain preserved.
**Verify:** `cargo test -p rho-engine engine::runner::turn::prune` -- passes.

#### Task 1.3: Wire cache-aware prune settings into turn preparation [2]
**Do:** In `crates/rho-engine/src/engine/runner/turn/prepare.rs`, configure `prune_historical_tool_outputs` with standard cache-protection parameters (`protect_recent_tokens: 30_000`, `prune_minimum_tokens: 20_000`, `min_prune_tokens: 50`).
**Tests:** Verify integration tests in `crates/rho-engine/src/engine/runner/turn/` retain recent turn tool outputs.
**Verify:** `cargo test -p rho-engine engine::runner::turn` -- passes.

#### Slice verification
- `cargo test -p rho-engine engine::runner::turn::prune` — 28 passed.
- `make complexity` and `make clippy` — passed cleanly. [Completed in 5815f3e]

---

### Slice 2: Lossless Tool Artifact Spilling via `DemotionHook`

#### Goal
Prevent loss of truncated command output by saving the complete output to `.rho/artifacts/{uuid}.log` using an `ArtifactDemotionHook` implementing `rig::memory::DemotionHook`, and injecting a reference pointer into the truncated tool result.

#### Acceptance criteria
- When tool output exceeds max bytes (50KB) or max lines (2,000), the complete output is saved to `.rho/artifacts/{uuid}.log`.
- The returned tool result includes head/tail previews and the exact path and line count of the saved artifact.
- Failures to write artifacts fall back gracefully to standard in-memory truncation.

#### Task 2.1: Implement `ArtifactDemotionHook` [3]
**Do:** Create `crates/rho-engine/src/tools/artifact.rs` defining `ArtifactDemotionHook` implementing `rig::memory::DemotionHook`. Implement directory creation (`.rho/artifacts/`), deterministic UUID naming, safe async file writing, and pointer message formatting.
**Tests:** Unit tests testing successful file creation, directory auto-creation, content integrity, and error recovery on unwritable paths.
**Verify:** `cargo test -p rho-engine tools::artifact` -- passes.

#### Task 2.2: Integrate artifact spilling into output truncation [2]
**Do:** In `crates/rho-engine/src/tools/truncate.rs`, update `truncate_output` to write the full content via `ArtifactDemotionHook` when truncation occurs, appending the artifact pointer to the truncated string.
**Tests:** Unit tests verifying that truncated bash and tool outputs contain the artifact notice and matching on-disk file.
**Verify:** `cargo test -p rho-engine tools::truncate` -- passes.

#### Task 2.3: Artifact storage cleanup / hygiene [2]
**Do:** Add cleanup logic in `crates/rho-engine/src/tools/artifact.rs` to prune artifacts older than 7 days or when total artifact storage exceeds 100MB.
**Tests:** Unit tests verifying that old artifact files are removed while recent files are preserved.
**Verify:** `cargo test -p rho-engine tools::artifact` -- passes.

#### Slice verification
- `cargo test -p rho-engine tools::artifact` — 4 passed.
- `make complexity` and `make clippy` — passed cleanly. [Completed in 5815f3e]

---

### Slice 3: Media Stripping on Summarization & Speculative Compaction

#### Goal
Eliminate user-visible turn latency during compaction by speculatively generating summary candidates in the background, and prevent summarizer context blowouts by stripping raw media parts.

#### Acceptance criteria
- Media parts (base64 image blocks) are replaced with metadata stubs before submitting history to the compaction LLM.
- When estimated context tokens reach 85% of the compaction threshold, a background task spawns to summarize the history snapshot up to the calculated cut point.
- If context reaches the threshold on the same branch, the pre-computed summary is committed immediately.
- If the session branches or user interrupts, the speculative summary is safely discarded without corrupting session state.

#### Task 3.1: Implement media stripping for summarization input [2]
**Do:** In `crates/rho-engine/src/engine/compactor/llm.rs`, sanitize input messages by replacing image/binary media blocks with text stubs `[Image: <mime> (<width>x<height>)]` before building the summarization prompt.
**Tests:** Unit tests verifying image messages are stripped in summarizer input while remaining intact in active history.
**Verify:** `cargo test -p rho-engine engine::compactor::llm` -- passes.

#### Task 3.2: Speculative compactor state and lead-band check [3]
**Do:** In `crates/rho-engine/src/engine/compactor/orchestrator.rs` and `auto_compact.rs`, add lead-band threshold detection (`context_tokens >= compaction_threshold * 0.85`) and `SpeculativeCompactionState` holding the active background join handle and snapshot node ID.
**Tests:** Unit tests verifying that lead band correctly triggers speculative initiation and ignores below-threshold contexts.
**Verify:** `cargo test -p rho-engine engine::compactor` -- passes.

#### Task 3.3: Instant commit on threshold breach and branch invalidation [2]
**Do:** In `AutoCompactHook`, if a valid speculative summary matching the current parent node exists when the hard compaction threshold is reached, commit it instantly; if the active branch diverged, discard and re-run.
**Tests:** Unit tests verifying immediate adoption on match, and fallback re-computation on branch divergence.
**Verify:** `cargo test -p rho-engine engine::runner::turn::auto_compact` -- passes.

#### Slice verification
- `cargo test -p rho-engine engine::runner::turn::auto_compact` — 12 passed.
- `cargo test -p rho-engine engine::compactor` — 31 passed.
- `make complexity` and `make clippy` — passed cleanly. [Completed in 5815f3e]

---

## Final verification

1. `make complexity` — Passed: all functions maintain Campbell cognitive complexity $\le 15$.
2. `make clippy` — Passed: zero warnings with `-D warnings`.
3. `cargo test -p rho-engine` — Passed across prune, compactor, artifact, and auto_compact.
4. `make fmt` — Formatting is clean.
