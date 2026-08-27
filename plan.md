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

### 2. Make configuration commands real — in progress

- [ ] Define typed configuration keys and values.
- [x] Implement persistence for `rho config <key> <value>` and display current settings when no key is supplied.
- [x] Persist updates atomically to `config.toml`.
- [ ] Preserve and test precedence: defaults -> file -> environment -> CLI.
- [x] Test invalid keys, invalid values, and persistence.

### 3. Centralize workspace path handling — in progress

- [x] Establish one workspace root for an engine/session and use it for read, write, edit, and Bash execution.
- [x] Introduce a shared `Workspace` path-resolution abstraction used by file tools and policy code.
- [x] Define behavior for absolute paths, `..`, missing parents, symlinks, and `.git` boundaries.
- [x] Define explicit exclusions for session files and config files.
- [x] Revalidate immediately before mutation.
- [x] Add traversal and boundary tests; add symlink coverage where platform permits.

### 4. Harden session persistence — in progress

- [x] Expose session state transitions rather than allowing arbitrary related record appends.
- [x] Verify atomicity of successful turns, cancellation, checkpoint save, and checkpoint promotion.
- [x] Keep session files and their containing directories private on Unix, including on resume and failed resume.
- [x] Test truncated records, malformed records, duplicate tool results, interrupted writes, and restart behavior.
- [ ] Add a session verification/diagnostic path if useful.

## Phase 2: Architecture

### 5. Narrow `AgentEngine`

- Extract construction into a builder/factory.
- Keep `AgentEngine` focused on coordinating a run.
- Move usage/quota tracking and context selection behind dedicated components.
- Keep provider creation, plugin loading, session management, and turn execution independently testable.

### 6. Introduce a typed tool registry

- Replace scattered tool-name string matching with tool descriptors and capabilities.
- Keep argument schema, capability, executor, display metadata, and policy metadata together.
- Make approval, audit, UI, and plugin hooks consume the same descriptor.

### 7. Improve structured errors

- Distinguish cancellation, authentication, configuration, provider, network, policy, tool, session, and budget failures.
- Preserve actionable context without leaking secrets.
- Test error mapping at CLI, runner, and tool boundaries.

## Phase 3: Extensibility and hardening

### 8. Define plugin permissions

- Add explicit plugin capabilities.
- Add deterministic ordering, timeout/error isolation, compatibility checks, and lifecycle audit events.
- Test plugin failures and conflicting plugin decisions.

### 9. Expand contract and fault-injection testing

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
