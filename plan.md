# rho Design Improvement Plan

## Goal

Move rho from a well-structured coding-agent CLI to a robust, maintainable, and security-conscious system whose important behavior is enforced by explicit contracts and verified by tests.

## Working rules

- Implement one bounded slice at a time.
- Prefer small changes with focused tests over broad rewrites.
- Preserve Rig as the model/agent transport boundary.
- Do not weaken safety checks to make tests pass.
- Do not add lint suppressions; refactor to satisfy the configured lints.
- Keep README behavior, CLI behavior, and implementation synchronized.

## Baseline verification

Before each slice, run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Record the baseline result in the change summary. If the baseline fails, fix or document that failure before adding unrelated changes.

## Phase 1: Correctness and safety

### 1. Unify approval policy — complete

- **Decision: trusted workspace.** In-workspace writes and edits run without approval; outside-workspace targets, protected metadata, mutating Bash, malformed calls, and unknown tools require approval unless `--auto-approve` is enabled.
- Keep `ExecutionClass`, `ApprovalCapability`, `ApprovalHook`, tool execution, README, and terminal UI consistent with this policy.
- Ensure authorization is checked immediately before the tool body executes.
- Ensure denial produces no side effect and a model-visible error.
- Add contract tests for read, workspace write, outside-workspace write, edit, Bash, network, malformed, and unknown calls.

### 2. Make configuration commands real — complete

- [x] Define typed configuration keys and values.
- [x] Implement persistence for `rho config <key> <value>` and display current settings when no key is supplied.
- [x] Persist updates atomically to `config.toml`.
- [x] Preserve and test precedence: defaults -> file -> environment -> CLI.
- [x] Test invalid keys, invalid values, and persistence.

### 3. Centralize workspace path handling — complete

- [x] Establish one workspace root for an engine/session and use it for read, write, edit, and Bash execution.
- [x] Introduce a shared `Workspace` path-resolution abstraction used by file tools and policy code.
- [x] Define behavior for absolute paths, `..`, missing parents, symlinks, and `.git` boundaries.
- [x] Define explicit exclusions for session files and config files.
- [x] Revalidate immediately before mutation.
- [x] Add traversal and boundary tests; add symlink coverage where platform permits.

### 4. Harden session persistence — complete

- [x] Expose session state transitions rather than allowing arbitrary related record appends.
- [x] Verify atomicity of successful turns, cancellation, checkpoint save, and checkpoint promotion.
- [x] Keep session files and their containing directories private on Unix, including on resume and failed resume.
- [x] Test truncated records, malformed records, duplicate tool results, interrupted writes, and restart behavior.
- [ ] Add a session verification/diagnostic path if useful.

## Phase 2: Architecture

### 5. Narrow `AgentEngine` — complete

- Extract construction into a builder/factory.
- Keep `AgentEngine` focused on coordinating a run.
- Move usage/quota tracking and context selection behind dedicated components.
- Keep provider creation, plugin loading, session management, and turn execution independently testable.

### 6. Introduce a typed tool registry — complete

- Replace scattered tool-name string matching with tool descriptors and capabilities.
- Keep argument schema, capability, executor, display metadata, and policy metadata together.
- Make approval, audit, UI, and plugin hooks consume the same descriptor.

### 7. Improve structured errors — complete

- Distinguish cancellation, authentication, configuration, provider, network, policy, tool, session, and budget failures.
- Preserve actionable context without leaking secrets.
- Test error mapping at CLI, runner, and tool boundaries.

## Phase 3: Extensibility and hardening

### 8. Define plugin permissions — complete

- Add explicit plugin capabilities.
- Add deterministic ordering, timeout/error isolation, compatibility checks, and lifecycle audit events.
- Test plugin failures and conflicting plugin decisions.

### 9. Expand contract and fault-injection testing — complete

Add offline tests for:

- denied operations and side-effect absence;
- cancellation and child-process cleanup;
- provider failures and authentication failures;
- checkpoint recovery;
- secret redaction on every persisted and rendered surface;
- symlink and path-race defenses;
- plugin timeout and failure isolation.

Use property-based tests for path resolution, Bash classification, canonical arguments, and JSONL validation where valuable.

## Per-slice verification gate

A slice is complete only when all of the following are true:

1. The intended behavior is documented or the existing documentation is updated.
2. Unit tests cover the new local logic.
3. At least one boundary/contract test covers subsystem interaction.
4. Failure and cancellation behavior are tested where applicable.
5. No credential sentinel appears in messages, sessions, checkpoints, artifacts, audits, errors, metrics, or rendered output.
6. No unintended filesystem, network, process, or provider side effects occur in tests.
7. `cargo fmt --check` passes.
8. `cargo clippy --all-targets -- -D warnings` passes.
9. `cargo test --all-targets` passes.
10. The diff is limited to the slice and its tests/documentation.

## Turn protocol

For each implementation turn:

1. State the slice being implemented.
2. Inspect the relevant code and existing tests.
3. Make the smallest coherent change.
4. Add or update tests before moving to the next slice.
5. Run the verification gate.
6. Report changed files, tests run, and any remaining risk.
7. Stop and reassess if the change reveals a conflicting contract or requires a new user decision.

## Completion criteria

The plan is complete when:

- approval semantics are explicit and consistent;
- configuration commands have honest, tested behavior;
- workspace operations have centralized boundary enforcement;
- session transitions are restart-safe and validated;
- `AgentEngine` has clear orchestration boundaries;
- tools expose typed capabilities and metadata;
- plugin permissions and failure behavior are explicit;
- cross-layer, fault-injection, redaction, and offline tests pass;
- the full repository verification gate passes from a clean checkout.

## Sources

### README accuracy verification (2026-08-29)

Verified README claims against source in crates/rho-engine (engine/provider/registry.rs, engine/quota.rs), crates/rho-core (provider.rs, error.rs, config.rs) and crates/rho-sdk (capability.rs). Findings:

- All documented providers, auth env vars, context ceilings (gpt-5/luna/codex=376k, claude/gemini=1000k, deepseek=128k; o1/o3=200k), the five-hour quota window, and ChatGPT & Copilot OAuth flows match the code exactly.
- Code catalog is a superset: README lists 8 providers; `ProviderId::ALL` has 12 — `xAi`, `mistral`, `cohere` are present in the catalog but undocumented. Plus the o1/o3 200k context ceiling.
- README under-reports (additive only); no factual inaccuracies.

### CLI verification (2026-08-29)

Verified against src/cli.rs and crates/rho-core/src/config/cli.rs. Findings:

- `rho` (REPL), `-p ""` one-shot, `--model`/`--provider`, `--resume <SESSION_ID>`, `rho config <key> <value>` (set) / `rho config` (show), and `rho plugin install <pkg>` / `rho plugin list` (plus `remove`, `inspect`) all exist and match the README.
- One factual discrepancy: example model ids `claude-sonnet-4-6` and `gpt-5.6-luna` are fictional. The default engine model is `claude-3-7-sonnet-20250219`; the CLI accepts `--model` as an opaque string with no catalog validation. CLI form is valid but the model string does not map to a real/default model.
- Default config: `src/cli.rs` dispatch, config merge precedence (file -> env -> CLI) in crates/rho-core/src/config/merge.rs:71-79.

### Interactive UI verification (2026-08-29)

Verified against src/ui/interactive/layout.rs, src/ui/interactive/controller.rs, src/repl/live.rs, src/repl/mod.rs, src/cli.rs. Findings:

- Live UI is gated on both stdin AND stdout being TTYs (`src/repl/live.rs:103-104`, `src/repl/mod.rs:101`); legacy reedline fallback + queue-free when either is non-TTY; one-shot print path in src/cli.rs:57-130 — all match README.
- Live editor uses the **normal** screen (alternate screen absent everywhere; controller.rs only enables raw mode), editor grows upward — matches README.
- Footer = `model | context | quota`, falling back to `activity.label()` only when all empty (`layout.rs:364-379`).
- Queued messages render as separate **lines above the editor**, not as a `N queued` footer segment (`layout.rs:175-193`).
- **Discrepancy:** the README example footer lines `⠋ thinking | gpt-5.6-luna | 2.9% (376k) | 74% (5d9h) | 2 queued` — the `2 queued` is not a footer segment in code, and the leading `⠋ thinking` word is not shown in the footer either (the working line shows `Working...`, not "thinking"; activity label appears in the footer only when model/context/quota are all empty). README prose "queued-message count" is also slightly off — it is a count of queued lines above the editor, not a footer count.
