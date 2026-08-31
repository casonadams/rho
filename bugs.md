# Bugs

These are tracked issues for other agents to pick up. Keep this file current as
bugs are fixed or resolved.

---

## [RESOLVED] BUG-1: Subagent model resolution cascade & executor wiring

### Status: Resolved

- `AppSubagentExecutor` is implemented in `src/platform.rs` and wired through
  `active_tools_with_auth` -> `ActiveToolSet::load_with_executor(config, base_dir, executor)`.
- In interactive, CLI, and RPC runtimes, subagents execute real agent turns via
  `AgentEngineBuilder` and live model APIs.
- Model resolution is implemented via `resolve_subagent_model` in
  `crates/rho-plugin-builtin/src/subagents/runner.rs` and called by
  `AppSubagentExecutor::execute` in `src/platform.rs`.
- Child subagents receive an isolated tool set loaded via `ActiveToolSet::load`
  (with `NoopExecutor`) to prevent unbounded recursive subagent spawning.

---

## [RESOLVED] BUG-2: `SubagentsConfig.default_model` is never read (dead config field)

### Status: Resolved

- `SubagentsConfig.default_model` is now actively used in the 4-tier model
  resolution cascade in `resolve_subagent_model`:
  1. `request.model_override` (e.g. `Agent(model: "...")` argument)
  2. `request.template.model` (e.g. template frontmatter / `[subagents.agents.<name>] model = "..."`)
  3. `config.subagents.default_model` (e.g. `[subagents] default_model = "..."`)
  4. `config.model` (root parent session model)
- Verified with unit tests in `crates/rho-plugin-builtin` and integration tests
  in `tests/subagents_integration.rs`.
