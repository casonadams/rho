# Specification: Passive Advisor / Reviewer Role

**Status**: Draft  
**Target Milestone**: v0.12.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`, `rho`

---

## 1. Problem Statement
When agents work through multi-step plans or large code changes, they can rush past subtle acceptance criteria, introduce unhandled error states, or break unrelated invariants. Catching these mistakes currently requires the user to manually review every turn or wait for a failed test.

## 2. Proposed Solution: Passive Advisor Model
Run a secondary reviewer model in the background:
1. **Isolated Context**: The advisor maintains its own lightweight context window and can run on a fast or cost-effective model (`models.advisor`, e.g. `gpt-4o-mini`, `gemini-2.5-flash`, or `clef-flash`).
2. **Turn Observation**: After the primary agent finishes a tool call or emits code, the advisor is triggered asynchronously.
3. **Inline Interventions**:
   - Quiet approval: Emits nothing or a check indicator.
   - Minor note: Surfaces a subtle dim info card.
   - Blocker / Concern: Renders an amber or red advisor card in the transcript warning that an acceptance criterion was missed or an error was swallowed.
4. **Primary Agent Steering**: The primary agent can inspect the advisor's notes on subsequent turns to self-correct before presenting the final answer to the user.

## 3. Implementation Steps
1. Add `advisor` configuration to `models` block in `config.toml`.
2. Implement asynchronous turn observer in `rho-engine/src/engine/runner/turn/`.
3. Render advisor feedback cards in `src/ui/` and `src/repl/live/`.
