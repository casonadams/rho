# Plan: Phase 1 — Subagent Decomposable Agency

## 1. Research

- **DeepMind Essay Reference**: *Artificial Symbiotic Intelligence* (Sep 2026) asserts that agents are decomposable assemblages of models, personas, tools, and scoped contexts. Monolithic prompts degrade under context bloat.
- **Rig Engine Interaction**: `rig` supports dynamic tools via `DynamicTool` and completions via `CompletionModel`. Subagents run an in-memory loop using the configured provider model handle with filtered tool sets.
- **Confinement & Cancellation**: Child executions must respect tokio cancellation tokens and process-wide `base_dir` sandbox boundaries without persisting ephemeral state to disk.

## 2. Reuse

- `crates/rho-engine/src/tools/builtin_tools/catalog.rs`: Reusing `BuiltinToolDeclaration`, `BuiltinToolKind`, and schema generators (`generated_schema`).
- `crates/rho-engine/src/tools/builtin_tools/mod.rs`: Reusing `dynamic_tool` helper and tool dispatch mechanics.
- `crates/rho-engine/src/adapter/rig/tools/mod.rs`: Reusing `into_dynamic_result`.
- `crates/rho-harness-core/src/error.rs`: Reusing `AppError::Policy` and `AppError::Tool`.
- `crates/rho-engine/src/engine/compactor/llm.rs`: Reusing existing `ModelHandle` for running completions.

## 3. Invariants and security boundaries

- **Role Confinement**: `Scout` and `Critic` roles must never have access to mutating tools (`write`, `edit`, `bash`). This boundary is structural (tools are excluded from the child tool list), not advisory.
- **Filesystem Confinement**: Subagents execute within the same workspace `base_dir` as the host session.
- **Disk Isolation**: Ephemeral child turns are kept strictly in-memory and are never serialized into `SessionManager`'s persistent turn history.
- **Recursion Ceiling**: Maximum nesting depth is strictly clamped (`max_depth = 1`). A child subagent cannot spawn further subagents.
- **Cancellation Propagation**: If the parent task is interrupted or dropped, child tokio tasks terminate immediately.

## 4. Quality gates

- **Formatting & Linting**: `cargo fmt --all -- --check` and `make clippy`.
- **Complexity Gate**: Cognitive complexity <= 15 per function via `cccc`.
- **CRAP Gate**: Workspace tests run under coverage; all modified functions must maintain CRAP <= 30 (`make crap`).
- **Full Verification**: `make all`.

## 5. Definition of done

- `SubagentRole` and `SubagentArgs` implemented and serialized with JSON schema.
- `SubagentTool` implemented, registered in `BuiltinToolCatalog`, and discoverable.
- Subagent executor runs isolated multi-turn loops and aggregates final summaries.
- Unit and integration tests verify:
  1. Role-based tool filtering (verifying absence of mutating tools for `Scout`/`Critic`).
  2. Max depth enforcement preventing recursion loops.
  3. Context isolation (parent history unpolluted).
  4. Token usage tracking propagation.
  5. Timeout/cancellation termination.
- All workspace checks pass with `make all`.

## 6. Assumptions

- The host session has an active, authenticated `ModelHandle` capable of generating completions.
- Subagent outputs can be serialized as plain text summaries for the parent tool output.

## 7. Risks

- **Risk**: Child agent loop entering an infinite tool call loop.
  - **Mitigation**: Enforce hard limit of `max_turns = 8` per subagent invocation.
- **Risk**: High token consumption from nested reasoning.
  - **Mitigation**: Child subagents use compact system prompts and max depth 1.

## 8. Dependencies

- None. Uses existing workspace dependencies (`rig`, `tokio`, `serde`, `serde_json`, `schemars`).

## 9. Decisions

- **Decision**: Execute subagents in-process via tokio tasks using clones of the active `ModelHandle` rather than spawning separate OS child processes.
  - *Context*: Subagents need fast startup and access to existing authenticated provider handles.
  - *Tradeoffs*: In-process execution is orders of magnitude faster and zero-overhead compared to OS process spawning, while remaining memory-isolated.
- **Decision**: Structurally partition tools by role at agent construction rather than filtering dynamically during dispatch.
  - *Context*: Safety and invariant enforcement.
  - *Tradeoffs*: Zero risk of LLM bypassing prompt instructions or tricking policy layers into executing unpermitted tools.

## 10. Out of scope

- Peer-to-peer remote agent networking over Iroh (Phase 2).
- Multi-party voting councils and consensus protocols (Phase 3).
- Split-screen TUI rendering (Phase 4).

---

## Vertical Slices

### Slice 1: Subagent Domain Types, Role Profiles & Catalog Declaration

- **Goal**: Establish the domain representations for subagent roles, arguments, role-specific prompts, and tool catalog definitions.
- **Acceptance criteria**: Satisfies REQ-001, REQ-002, REQ-003.
- **Tasks**:
  #### Task 1.1: Define SubagentRole, SubagentArgs, and role permissions [2]
  **Do:** Create `crates/rho-engine/src/tools/subagent/types.rs` defining `SubagentRole` (`Scout`, `Critic`, `Planner`, `General`), `SubagentArgs`, and helper methods `allowed_tools(&self) -> &[&str]`.
  **Covers:** REQ-002, REQ-003.
  **Tests:** Unit tests verifying role serialization, deserialization, and allowed tool rosters.
  **Verify:** `cargo test -p rho-engine subagent::types` -- passes with 100% assertions.

  #### Task 1.2: Register subagent in BuiltinToolCatalog [2]
  **Do:** Add `subagent` declaration to `crates/rho-engine/src/tools/builtin_tools/catalog.rs` with documentation, guidelines, and `schema: generated_schema::<SubagentArgs>`.
  **Covers:** REQ-001.
  **Tests:** Unit test in `catalog.rs` verifying `subagent` declaration existence and schema validity.
  **Verify:** `cargo test -p rho-engine tools::builtin_tools` -- passes.

- **Slice verification**: `cargo test -p rho-engine subagent` and `cargo test -p rho-engine builtin_tools`.

---

### Slice 2: Subagent Isolated Execution Loop & Engine Tool

- **Goal**: Implement the isolated in-memory turn runner and `SubagentTool` that executes child loops and returns condensed results.
- **Acceptance criteria**: Satisfies REQ-004, REQ-005, REQ-006, REQ-007, REQ-008, AC-001, AC-002, AC-003, AC-004.
- **Tasks**:
  #### Task 2.1: Implement SubagentRunner for isolated turn execution [3]
  **Do:** Create `crates/rho-engine/src/tools/subagent/runner.rs` containing `SubagentRunner`. Builds an isolated `rig::agent::Agent`, populates role instructions, sets `max_turns = 8`, runs completions, dispatches permitted tools, and compiles a final textual synthesis.
  **Covers:** REQ-004, REQ-005, REQ-007, AC-001.
  **Tests:** Unit tests with mock model verifying loop termination on final answer and `max_turns` boundary.
  **Verify:** `cargo test -p rho-engine subagent::runner` -- passes.

  #### Task 2.2: Implement SubagentTool with depth enforcement [3]
  **Do:** Implement `EngineTool` for `SubagentTool` in `crates/rho-engine/src/tools/subagent/mod.rs`. Tracks recursion depth (`depth: usize`, default 0), rejecting invocation when `depth >= 1`.
  **Covers:** REQ-001, REQ-006, AC-004.
  **Tests:** Unit tests verifying depth 0 succeeds and depth 1 fails with descriptive policy error.
  **Verify:** `cargo test -p rho-engine subagent::tests` -- passes.

  #### Task 2.3: Wire SubagentTool into BuiltinToolCatalog instantiation [2]
  **Do:** Wire `SubagentTool` into `crates/rho-engine/src/tools/builtin_tools/mod.rs` so that `AgentEngineBuilder` registers it in the available tool roster.
  **Covers:** REQ-001, REQ-008.
  **Tests:** Integration test checking that `subagent` is present in `AgentEngine::tool_names()`.
  **Verify:** `cargo test -p rho-engine engine::tests` -- passes.

- **Slice verification**: `cargo test -p rho-engine subagent` and `make crap`.

---

### Slice 3: Verification, End-to-End Integration & Quality Bar

- **Goal**: Comprehensive end-to-end integration testing, CRAP <= 30 gating, cognitive complexity <= 15 gating, and docs.
- **Acceptance criteria**: Satisfies Definition of Done.
- **Tasks**:
  #### Task 3.1: Add end-to-end subagent execution integration test [3]
  **Do:** Add integration test in `crates/rho-engine/tests/` or `crates/rho-engine/src/tools/subagent/` simulating parent calling a `Scout` subagent, confirming that parent context history remains clean while scout discovers files.
  **Covers:** AC-001, AC-002, AC-003.
  **Tests:** Integration test asserting parent history contains only the `subagent` tool call and final output, not child intermediate turns.
  **Verify:** `cargo test -p rho-engine subagent` -- passes.

  #### Task 3.2: Verify repository quality bar [2]
  **Do:** Run `make all` to ensure format, clippy, cognitive complexity <= 15 (`cccc`), quality regressions (`ripwire`), and test code coverage CRAP <= 30 pass.
  **Covers:** Quality gates.
  **Tests:** All workspace tests.
  **Verify:** `make all` -- all 5 checks succeed.

---

## Final verification

- `cargo fmt --all -- --check`
- `make clippy`
- `make complexity`
- `make crap`
- `make all`
