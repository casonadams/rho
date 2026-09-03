---
name: plan
description: Explore the codebase and deliver a vertical-slice implementation plan with acceptance criteria.
argument-hint: "<task-description-or-spec>"
---

# Plan Implementation Workflow

When planning an implementation:

## 1. Gather & Explore
- Inspect relevant code, existing utilities, platform capabilities, and dependencies.
- Avoid assumptions; verify existing patterns and architecture before modifying.
- Investigate code reuse before proposing new dependencies or custom implementations.

## 2. Clarify Ambiguities
- Resolve blockers and architectural choices before writing code.
- If requirements are ambiguous, clarify tradeoffs with concise, structured options.

## 3. Plan in Vertical Slices
Organize work into thin vertical slices delivering end-to-end user-observable behavior:
- **Goal**: Clear, single-sentence objective.
- **Acceptance Criteria**: Observable conditions for completion.
- **Tasks**: Concrete, sequential steps scoring effort with Fibonacci numbers (1, 2, 3, 5).
- **Verification**: Runnable command (`cargo test`, `cargo clippy`) with expected outputs.
