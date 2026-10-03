# Specification: Role-Based Model Routing & Presets

**Status**: Draft  
**Target Milestone**: v0.10.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`, `rho`

---

## 1. Problem Statement
Users currently configure a single active model (`--model` or `models.default`). However, different tasks have vastly different economics and capability requirements:
- **Fast / Cheap tasks (`smol`)**: Code exploration, simple search queries, one-line edits, commit message generation, subagent tasks.
- **Deep reasoning tasks (`slow` / `reasoning`)**: Hard architectural bugs, concurrency edge cases, deep refactoring plans.
- **Planning tasks (`plan`)**: Generating execution outlines without mutating files.
- **Passive Review (`advisor`)**: Checking diffs in the background.

Constantly running a top-tier frontier model for everything wastes money, quota, and latency.

## 2. Proposed Solution: First-Class Named Roles

Define standard role aliases in configuration (`config.toml`):

```toml
[models]
default = "anthropic/claude-3-7-sonnet"
smol = "gemini/gemini-2.5-flash"
slow = "openai/o3-mini"
plan = "anthropic/claude-3-7-sonnet"
commit = "ollama/qwen2.5-coder:7b"
advisor = "openai/gpt-4o-mini"
```

### CLI Overrides
- `rho --smol` -> Launches session using the configured `smol` model.
- `rho --slow` -> Launches session using the configured `slow` reasoning model.
- `rho --plan` -> Launches in plan mode with the configured `plan` model.

### In-REPL Role Toggles
- `Ctrl+P` or `/role`: Cycle or select active role directly in the TUI footer.
- The footer metric updates to display both the role and model name: `[smol: gemini-2.5-flash]`.

## 3. Implementation Steps
1. Add `Role` enum to `rho-harness-core/src/config/`: `Default`, `Smol`, `Slow`, `Plan`, `Commit`, `Advisor`.
2. Update config parser and CLI clap definitions with `--smol`, `--slow`, `--plan`.
3. Wire role resolution to `ModelRegistry` in `rho-engine`.
4. Update UI status bar in `src/repl/live/` to show the active role badge.
