# Environmental & Mesh Intelligence (Sovereign Agents, Native LSP, & Universal VFS) Spec

## Status

Draft

## Problem

Modern agentic coding harnesses face three acute environmental limitations when moving beyond simple local text edits:
1. **Blind Semantic Inspection**: Agents lack real-time language server feedback. Finding definitions, checking references, or validating compile diagnostics currently forces slow, costly compiler executions via `bash` (`cargo check`, `tsc`, `pyright`), resulting in delayed error recovery loops and brittle multi-file renames.
2. **Context Fragmentation & Tool Sprawl**: Non-filesystem context—such as git branch diffs, merge conflict hunks, and GitHub pull requests or issues—currently requires external bash scripts or ad-hoc custom tools. This bloats prompt schemas, consumes tool selection tokens, and increases the chance of hallucinated CLI flags.
3. **Single-Host Compute Confinement**: Agents are confined to a single host machine and process. Heavy compilation, deep evaluation, or specialized model inference (such as local GPU clusters or isolated Linux sandbox environments) cannot be securely delegated to sovereign remote nodes without exposing public HTTP endpoints, maintaining centralized relay servers, or compromising secrets.

## Users and stakeholders

- **Software Engineers**: Need agents that can inspect diagnostics and definitions without breaking the flow, resolve git diffs and conflicts cleanly, and delegate heavy workloads to separate compute nodes.
- **Agent Orchestrators**: Require clean boundaries, minimal tool schemas, and cryptographically verified task delegation across heterogeneous systems.

## Goals

- Provide a first-class `lsp` engine tool for instant semantic queries (diagnostics, definition, references, document symbols, and workspace symbol renames) with auto-detection of common language servers (`rust-analyzer`, `typescript-language-server`, `gopls`, `pyright`).
- Provide unified Virtual File System (VFS) URI resolution in the standard `read` tool:
  - `diff://<target>` for git diffs against working tree, staged index, or commits.
  - `conflict://[file]` for detecting and inspecting git merge conflicts.
  - `pr://<id>[/diff]` and `issue://<id>` for GitHub context fetching without shell commands.
- Provide sovereign peer-to-peer agent networking over Iroh QUIC streams (ASI Milestone Phase 2):
  - Agent ALPNs: `rho/agent/task/v1` (task delegation) and `rho/agent/eval/v1` (code critique).
  - Cryptographic capability tickets with fine-grained permissions: `ReadOnly`, `Propose`, and `Full`.
  - CLI commands `rho agent serve` and `rho agent run` to pair nodes and distribute agent execution with zero telemetry or central brokers.

## Non-goals

- Implementing full text editors or LSP servers from scratch; `rho` acts purely as an LSP client talking to existing standard language server executables.
- Multi-agent voting councils or consensus quorums (reserved for ASI Phase 3).
- Centralized relay infrastructure or cloud-hosted agent brokers.

## Current behavior

- **LSP**: Basic data structures exist in `crates/rho-engine/src/lsp/mod.rs` with `auto_detect_command`, but no active JSON-RPC process lifecycle manager, diagnostic cache, or engine tool exists.
- **VFS**: Basic URI parsing and local git diff/conflict helpers exist in `crates/rho-engine/src/tools/read/vfs.rs`, but remote PR/issue resolution is a stub, and git diffs do not support staged index or revision range syntax directly.
- **P2P Networking**: `crates/rho-harness-core/src/collab/` implements human terminal sharing via Iroh under `COLLAB_ALPN` (`rho/collab/v1`), but does not support headless autonomous agent task delegation, agent RPC protocols, or agent capability tokens.

## Desired behavior

- The agent can invoke `lsp` with operations `diagnostics`, `definition`, `references`, `symbols`, and `rename`. The harness launches or reuses the appropriate language server, translates coordinates, and returns structured code references or compiler diagnostics.
- The agent calls `read` with URI schemes:
  - `diff://HEAD` returns working tree changes.
  - `diff://staged` returns staged changes.
  - `diff://main...feature` returns revision comparison diffs.
  - `conflict://` lists active merge conflict files, and `conflict://path/to/file` extracts conflicted hunks.
  - `pr://123` or `gh://owner/repo/pull/123` formats pull request title, description, comments, and changed files.
- An operator can run `rho agent serve --cap propose` on a Linux build box or GPU server to emit an encrypted ticket (`rho://agent/...`). The lead `rho` harness can delegate a subagent task to that peer node over an end-to-end encrypted QUIC connection via Iroh NAT traversal.

## Requirements

### Virtual File System (VFS) Schemes in `read`
- **REQ-001**: The `read` tool must detect and dispatch URI schemes `diff://`, `conflict://`, `pr://`, `issue://`, and `gh://`.
- **REQ-002**: `diff://` must support empty/`HEAD` (working tree changes), `staged` (`--staged`), and git revision ranges (`refA...refB`).
- **REQ-003**: `conflict://` must return a list of unmerged files when no path is given, or full file content with conflict markers when a path is specified.
- **REQ-004**: `pr://` and `issue://` must fetch markdown-formatted representations of GitHub pull requests or issues using available environment tokens (`GITHUB_TOKEN` or `gh` auth) or fallback gracefully with clean guidance.
- **REQ-005**: All VFS reads must respect existing `offset`, `limit`, and line-numbering flags from `ReadArgs`.

### Native Language Server Protocol (`lsp`) Tool
- **REQ-006**: The engine must provide an `lsp` builtin tool accepting `operation`, `path`, `line`, `character`, and `new_name`.
- **REQ-007**: The LSP client must auto-detect project servers (`rust-analyzer` for Cargo projects, `typescript-language-server` for Node/TS projects, `gopls` for Go, `pyright-langserver` for Python) and gracefully handle server absence.
- **REQ-008**: The `lsp` tool must support operations:
  - `diagnostics`: retrieve compile errors and warnings for the file or workspace.
  - `definition`: resolve definition locations (`path:line:col`).
  - `references`: list all symbol usages across the workspace.
  - `symbols`: list outline/symbols in the specified document.
  - `rename`: calculate workspace-wide edits for renaming a symbol.
- **REQ-009**: LSP client communication must be asynchronous, communicate over standard input/output with Content-Length framing, and enforce request timeouts (default 10s).

### Sovereign Agent Mesh over Iroh
- **REQ-010**: Define dedicated Iroh ALPNs: `rho/agent/task/v1` for task execution and `rho/agent/eval/v1` for adversarial evaluation.
- **REQ-011**: Implement cryptographic agent capability tickets (`rho-agent://...`) encoding endpoint ID, optional relay URL, capability level (`ReadOnly`, `Propose`, `Full`), and secret key.
- **REQ-012**: Enforce capability invariants on worker nodes:
  - `ReadOnly`: Worker only runs non-mutating discovery tools (`read`, `fd`, `rg`, `lsp`).
  - `Propose`: Worker can generate diffs and read files, but cannot execute arbitrary shell commands (`bash`).
  - `Full`: Worker has full access within its local sandbox.
- **REQ-013**: Implement CLI entry points:
  - `rho agent serve`: Start a sovereign worker node listening for incoming Iroh QUIC connections.
  - `rho agent run <ticket> --prompt <prompt>`: Connect to a remote worker node, stream turn execution events, and return the final synthesis.

## Invariants and security boundaries

- **Capability Enforcement**: A remote peer holding a `ReadOnly` or `Propose` capability ticket must never be permitted to execute mutating operations (`write`, `edit`, `bash`) on the worker. Verification must happen at the protocol receiver layer, not via client-side trust.
- **Filesystem Confinement**: VFS operations must not traverse outside the git root repository or execute arbitrary shell injection through git arguments.
- **Zero Central Telemetry**: All peer discovery and connection negotiation occurs directly over Iroh's Ed25519-authenticated QUIC transport and distributed DERP relays without central telemetry, user tracking, or third-party cloud brokers.

## Definition of done

- All new modules pass unit tests and integration tests.
- `BuiltinToolCatalog` registers `lsp` and exposes accurate schemas.
- `read` tool seamlessly handles `diff://`, `conflict://`, `pr://`, and `issue://` with standard line slicing and numbering.
- Agent task delegation completes end-to-end over an Iroh QUIC loopback stream between host and worker with capability verification.
- `make all` passes cleanly: zero fmt errors, zero clippy warnings, cognitive complexity <= 15 via `cccc`, quality regression check via `ripwire`, and CRAP <= 30 gating.

## Acceptance criteria

- AC-001: Calling `read` with `diff://staged` returns only staged index modifications with accurate line numbers.
- AC-002: Calling `read` with `conflict://` in a repository with merge conflicts lists unmerged files; requesting a specific file returns conflict hunks.
- AC-003: Calling `lsp` with `diagnostics` returns compiler errors or clean status without manual `cargo check` execution.
- AC-004: Calling `lsp` with `definition` on a known symbol returns its exact workspace location.
- AC-005: Running `rho agent serve` generates a valid `rho-agent://` ticket. A client executing `rho agent run` with that ticket successfully streams remote progress and gathers the synthesized response.
- AC-006: A worker receiving a task on a `ReadOnly` ticket strictly refuses to invoke mutating tools even if prompted by the model.

## Edge cases

- Missing LSP binary: If `rust-analyzer` or `pyright` is not installed on the host machine, the `lsp` tool returns a structured error suggesting installation rather than crashing.
- Non-git workspace: `diff://` and `conflict://` return an informative error when executed outside a git repository.
- Network disconnection during agent mesh execution: The client detects QUIC stream termination, cancels child listeners, and returns an actionable transport error.

## Constraints

- Maximum function cognitive complexity <= 15 (`cccc`).
- CRAP score <= 30 for all modified and new functions.
- Fully non-blocking async I/O via `tokio`.

## Risks and mitigations

- **Risk**: LSP server initialization delays or hangs on large repositories.
  - **Mitigation**: Lazy initialization with an in-flight timeout (10s), background server caching, and health-check pings.
- **Risk**: Remote execution security vulnerabilities on worker nodes.
  - **Mitigation**: Default to `ReadOnly` capability; strict server-side capability boundary checking before tool dispatch.

## Open questions

- None (all blocker requirements and capability models are resolved).

## References

- DeepMind: *Artificial Symbiotic Intelligence: Agents, AGI and the Orchestration of Many Minds* (September 2026).
- Iroh Protocol Documentation: `https://iroh.computer`
- Language Server Protocol Specification v3.17: `https://microsoft.github.io/language-server-protocol/`
