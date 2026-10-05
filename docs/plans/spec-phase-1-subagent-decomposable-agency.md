# Phase 1: Subagent Decomposable Agency Spec

## Status

Approved

## Problem

Today, `rho` operates as a strictly linear, monolithic agent. Every exploratory query (`fd`, `rg`), trial edit, or side investigation consumes the primary agent's single context window. This architecture has three critical failure modes:
1. **Context Bloat & Attention Rot**: Large tool outputs (such as directory listings or search matches) permanently occupy context memory, diluting model focus and degrading reasoning on subsequent turns.
2. **Monolithic Incompetence**: A single system prompt and model configuration attempts to be simultaneously an exploratory scout, a cautious code reviewer, a high-level architect, and an aggressive implementer.
3. **Serial Bottlenecks**: The agent cannot fork an isolated investigation while maintaining its high-level coordination state.

## Users and stakeholders

- **Developer / Operator**: Expects faster, more accurate problem-solving without runaway token costs or context window exhaustion.
- **Agent Orchestrator (Parent Engine)**: Needs a reliable mechanism to delegate scoped subtasks and receive condensed, high-signal results.

## Goals

- Implement a native `subagent` tool allowing the lead agent to spawn scoped child agents with tailored role profiles (`scout`, `critic`, `planner`, `general`).
- Isolate context: child agent tool invocations, intermediate reasoning, and transient errors remain confined to the child's temporary session and do not pollute the parent's context window.
- Enforce strict role-based tool gating: `scout` and `critic` roles have zero access to mutating tools (`write`, `edit`, `bash`).
- Support configurable execution bounds: maximum turn depth (default 8) and recursion depth limit (max 1 level of subagent nesting) to prevent runaway execution loops.
- Propagate cancellation: interrupting or cancelling the parent turn immediately aborts all active subagent tasks.

## Non-goals

- Remote or cross-machine peer-to-peer agent networking (deferred to Phase 2 with Iroh ALPNs).
- Autonomous adversarial consensus voting (deferred to Phase 3).
- Graphical or nodal multi-pane TUI rendering (deferred to Phase 4; Phase 1 reports subagent lifecycle events through the existing UI event stream).

## Current behavior

- `AgentEngine` contains a single `Agent` instance (`crates/rho-engine/src/engine/mod.rs`).
- All tool calls execute directly in the main turn loop (`crates/rho-engine/src/engine/runner/turn/`).
- All tool inputs and outputs are appended directly to the session's shared message history (`rig::completion::Message`).
- There is no mechanism to branch context or execute a sub-prompt in an isolated sandbox.

## Desired behavior

- The engine exposes a built-in `subagent` tool to the model (when enabled in configuration).
- The parent agent invokes `subagent` with:
  - `role`: The specialized agent profile (`scout`, `critic`, `planner`, `general`).
  - `prompt`: The specific goal or question to solve.
  - `context_slice`: Optional contextual text or file references to prime the subagent.
- The subagent runs its own isolated turn loop up to `max_turns`, utilizing only the tools allowed for its role.
- When the subagent reaches a final answer or concludes, only the condensed summary is returned as the tool output to the parent agent.
- Intermediate tool calls, logs, and token usage are tracked and aggregated into the parent session's `SessionUsageTotals` without bloating the parent message history.

## Requirements

- REQ-001: The system shall provide a `SubagentTool` implementing `EngineTool` that can be registered in `BuiltinToolCatalog`.
- REQ-002: The system shall define a `SubagentRole` enum supporting `Scout`, `Critic`, `Planner`, and `General`.
- REQ-003: `Scout` and `Critic` roles shall be cryptographically and structurally restricted to read-only inspection tools (`read`, `fd`, `rg`, `web_fetch`, `web_search`), rejecting any mutating tools (`edit`, `write`, `bash`).
- REQ-004: Each subagent invocation shall execute with an independent context window (in-memory `Message` list) initialized with role-specific system instructions and the provided prompt/context slice.
- REQ-005: The subagent loop shall terminate upon reaching a final text completion, encountering an unrecoverable error, or hitting `max_turns` (configurable, default 8).
- REQ-006: Subagent recursion depth shall be strictly enforced with a `max_depth` parameter (default 1). If a subagent attempts to invoke `subagent` at or above `max_depth`, the tool call shall fail immediately with a descriptive policy error.
- REQ-007: Subagent cancellation tokens shall be linked to the parent turn's cancellation token; triggering parent cancellation must immediately abort child execution.
- REQ-008: Subagent token usage (input tokens, output tokens, reasoning tokens) shall be added to the parent engine's usage tracker upon completion.

## Invariants and security boundaries

- Subagents must strictly respect the filesystem confinement and working directory boundaries (`base_dir`) of the host process.
- Role-based tool gating is enforced at tool construction time, not via prompt suggestion. A `Scout` agent does not have `write` or `bash` in its registered tool set.
- Subagents must not access or mutate the parent's `SessionManager` disk files; they run in-memory and discard ephemeral message histories upon completion.

## Definition of done

- Unit and integration tests verify:
  1. Subagent tool registration and parameter schema.
  2. Role-based tool filtering (`Scout` cannot execute `write`).
  3. Isolated context execution (parent history remains clean).
  4. Recursion depth limit enforcement.
  5. Parent cancellation propagation.
  6. Token usage accumulation.
- All code passes `make all` (`fmt-check`, `clippy`, `cccc` cognitive complexity <= 15, `quality` via `ripwire`, and `crap` test coverage <= 30 gating).

## Acceptance criteria

- AC-001: Given a parent session, when the model invokes `subagent(role: "scout", prompt: "list files matching *.rs")`, then the scout executes using read-only tools and returns the file list without polluting the parent transcript with intermediate tool calls.
- AC-002: Given a `scout` subagent, when asked to modify a file, then the subagent cannot invoke `write` or `edit` because those tools are not registered in its tool catalog.
- AC-003: Given a running subagent, when the parent turn is cancelled (e.g. via Escape/interrupt), then the subagent's execution future is dropped and promptly terminates.
- AC-004: Given an active subagent at depth 1, when it attempts to call `subagent`, then the call returns an error indicating maximum recursion depth exceeded.

## Edge cases

- Subagent reaches `max_turns` without emitting a final answer: return a clean timeout/exhaustion summary with the last known observation instead of panicking.
- Model quota exhaustion or provider 429 during child execution: child reports error gracefully to parent tool result so parent can retry or fallback.
- Empty prompt or whitespace-only prompt: rejected with invalid args error before spawning model.

## Constraints

- Zero external network requests or third-party dependencies during unit tests; tests must use in-memory fakes or mock model completions.
- Strictly adhere to `cccc` cognitive complexity <= 15 and CRAP <= 30 per function.

## Risks and mitigations

- Risk: Nested subagents consuming excessive API tokens or quota.
  - Mitigation: Enforce default `max_turns = 8` and `max_depth = 1`.
- Risk: Deadlock if subagents share locked state with parent engine.
  - Mitigation: Subagents operate on clones or read-only shared references; mutable state is strictly passed by message or accumulated at task join.

## References

- DeepMind Essay: *Artificial symbiotic intelligence: Agents, AGI and the orchestration of many minds* (Sep 2026).
- `docs/plans/milestones-artificial-symbiotic-intelligence.md`
- `crates/rho-engine/src/tools/`
- `crates/rho-engine/src/engine/runner/turn/`
