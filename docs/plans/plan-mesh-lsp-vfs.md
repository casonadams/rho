# Implementation Plan: Environmental & Mesh Intelligence (Sovereign Agents, Native LSP, & Universal VFS)

Based on the specification in `docs/plans/spec-mesh-lsp-vfs.md`, this plan details the vertical slices to deliver universal VFS schemes, native LSP semantic intelligence, and sovereign P2P agent delegation over Iroh.

---

## 1. Research

- **Iroh P2P Networking**: `iroh` (v1.3) is already integrated into `rho-harness-core` for terminal collaboration (`rho/collab/v1`). It provides automatic NAT traversal, Ed25519 identity, and reliable QUIC streams without central brokers.
- **Language Server Protocol (LSP)**: LSP servers communicate over stdio using HTTP-like header framing (`Content-Length: <n>\r\n\r\n<json>`). Standard servers (`rust-analyzer`, `typescript-language-server`, `gopls`, `pyright-langserver`) follow this protocol.
- **Virtual File System Schemes**: Pre-existing parser in `crates/rho-engine/src/tools/read/vfs.rs` provides basic URI parsing; expanding this allows the agent to inspect git diffs, merge conflicts, and GitHub discussions through the existing line-offset and line-numbering pipeline of `read`.

## 2. Reuse

- `crates/rho-harness-core/src/collab/ticket.rs` & `crypto.rs`: Reuse patterns for URL-safe base64 ticket encoding, HKDF key derivation, and Ed25519 endpoint address parsing.
- `crates/rho-engine/src/tools/read/mod.rs` & `format.rs`: Reuse existing `format_content` for pagination, line numbering, and artifact emission.
- `crates/rho-engine/src/tools/subagent/runner.rs`: Reuse isolated subagent runner for executing tasks on remote worker nodes.
- `crates/rho-engine/src/tools/builtin_tools/catalog.rs`: Reusing `BuiltinToolDeclaration` and `generated_schema`.

## 3. Invariants and security boundaries

- **Worker Node Capability Confinement**: The worker enforces capability levels (`ReadOnly`, `Propose`, `Full`) before tool execution. A `ReadOnly` ticket strictly disallows mutating tools (`write`, `edit`, `bash`) at the transport receiver boundary.
- **LSP Process Containment**: Language server processes are spawned as child processes of the current user within `base_dir`, adhering to the workspace directory.
- **Zero Command Injection in VFS**: All git arguments passed in `diff://` or `conflict://` are sanitized or passed as discrete process arguments without shell expansion.

## 4. Quality gates

- `cargo fmt --all -- --check`
- `make clippy`
- `make complexity` (cognitive complexity <= 15 via `cccc`)
- `make quality` (quality delta checks via `ripwire`)
- `make crap` (test coverage and CRAP score <= 30 gating)
- `make all`

## 5. Definition of done

- `read` supports `diff://HEAD`, `diff://staged`, `diff://<ref>`, `conflict://[path]`, and `pr://<id>`.
- `lsp` tool is registered, auto-detects language servers, and supports `diagnostics`, `definition`, `references`, `symbols`, and `rename`.
- `rho agent serve` and `rho agent run` successfully distribute agent tasks over an encrypted Iroh QUIC stream with capability enforcement.
- All workspace checks pass with `make all`.

## 6. Assumptions

- Developers using the `lsp` tool have the relevant language server (`rust-analyzer`, `typescript-language-server`, etc.) installed on their `$PATH` when invoking LSP features for that language.
- Remote worker nodes have network connectivity to the Iroh DERP relay system for NAT traversal.

## 7. Risks

- **Risk**: Stalled LSP processes during project indexing.
  - **Mitigation**: Implement 10-second request timeouts and non-blocking background initialization.
- **Risk**: Malicious prompt attempting to breach capability permissions on remote worker nodes.
  - **Mitigation**: Capability filtering is structural and verified by the worker daemon before registering tools for the turn runner.

## 8. Dependencies

- No new external dependencies required; uses existing workspace dependencies (`iroh`, `tokio`, `serde`, `serde_json`, `ring`, `base64`).

## 9. Decisions

- **Decision**: Embed the LSP client as an asynchronous in-process JSON-RPC handler rather than requiring an external LSP daemon.
  - *Context*: Minimizes operational overhead and external setup for users.
- **Decision**: Reuse Iroh QUIC streams and ALPN negotiation for agent task delegation rather than building a custom HTTP/WebSocket daemon.
  - *Context*: Leverages battle-tested NAT-punching and end-to-end Ed25519 encryption already present in `rho`.

## 10. Out of scope

- Multi-agent consensus voting algorithms (reserved for Phase 3).
- Graphical TUI node visualizers (reserved for Phase 4).

---

## Vertical Slices

### Slice 1: Universal Virtual File System Schemes in `read` (`vfs`)

- **Goal**: Enable `read` to resolve `diff://`, `conflict://`, `pr://`, and `issue://` with full line-numbering and range support.
- **Acceptance criteria**: Satisfies REQ-001, REQ-002, REQ-003, REQ-004, REQ-005, AC-001, AC-002.
- **Tasks**:
  #### Task 1.1: Enhance git diff resolver for staged and revision comparisons [2]
  **Do:** Update `crates/rho-engine/src/tools/read/vfs.rs` to support `diff://staged` (`--staged`), `diff://HEAD`, and git revision ranges (`diff://main...feature`).
  **Covers:** REQ-001, REQ-002, AC-001.
  **Tests:** Unit tests verifying resolution of staged changes and revision diffs.
  **Verify:** `cargo test -p rho-engine tools::read::vfs` -- passes.

  #### Task 1.2: Implement merge conflict hunk extractor [2]
  **Do:** In `vfs.rs`, parse unmerged files for `conflict://` and extract specific conflict sections with clear annotations (`OURS`, `THEIRS`, `BASE`) when a file path is provided.
  **Covers:** REQ-003, AC-002.
  **Tests:** Unit tests with mock conflict files verifying hunk extraction.
  **Verify:** `cargo test -p rho-engine tools::read::vfs` -- passes.

  #### Task 1.3: Add GitHub PR and Issue context resolver [2]
  **Do:** In `vfs.rs`, resolve `pr://<id>`, `pr://<id>/diff`, and `issue://<id>` by reading local `gh` CLI or environment tokens when available, returning structured Markdown summaries.
  **Covers:** REQ-004, REQ-005.
  **Tests:** Unit tests for GitHub URI formatting and fallback error handling.
  **Verify:** `cargo test -p rho-engine tools::read::vfs` -- passes.

- **Slice verification**: `cargo test -p rho-engine tools::read` and `make crap`.

---

### Slice 2: Native Language Server Protocol (`lsp`) Tool

- **Goal**: Implement the asynchronous LSP JSON-RPC client and expose the `lsp` tool in `BuiltinToolCatalog`.
- **Acceptance criteria**: Satisfies REQ-006, REQ-007, REQ-008, REQ-009, AC-003, AC-004.
- **Tasks**:
  #### Task 2.1: Implement LSP JSON-RPC protocol framing and client [3]
  **Do:** Create `crates/rho-engine/src/lsp/client.rs` implementing async LSP communication over stdio with Content-Length header framing, message serialization, and response matching.
  **Covers:** REQ-009.
  **Tests:** Unit tests for header serialization/deserialization and request framing.
  **Verify:** `cargo test -p rho-engine lsp::client` -- passes.

  #### Task 2.2: Implement LSP process lifecycle manager [3]
  **Do:** Create `crates/rho-engine/src/lsp/manager.rs` managing background language server processes, tracking project roots, initializing capabilities (`initialize`, `initialized`), and caching active instances.
  **Covers:** REQ-007, REQ-009.
  **Tests:** Unit tests verifying auto-detection and initialization timeout guards.
  **Verify:** `cargo test -p rho-engine lsp::manager` -- passes.

  #### Task 2.3: Implement LspTool and register in catalog [3]
  **Do:** Create `crates/rho-engine/src/tools/lsp/` implementing `EngineTool` for `lsp` with operations (`diagnostics`, `definition`, `references`, `symbols`, `rename`), and register in `BuiltinToolCatalog`.
  **Covers:** REQ-006, REQ-008, AC-003, AC-004.
  **Tests:** Unit tests verifying tool execution and argument parsing.
  **Verify:** `cargo test -p rho-engine tools::lsp` -- passes.

- **Slice verification**: `cargo test -p rho-engine lsp` and `make crap`.

---

### Slice 3: Sovereign Agent Protocol & Capability Mesh over Iroh

- **Goal**: Define P2P agent protocol ALPNs, capability tokens, and secure framing over Iroh QUIC streams.
- **Acceptance criteria**: Satisfies REQ-010, REQ-011, REQ-012, AC-006.
- **Tasks**:
  #### Task 3.1: Define Agent ALPNs and AgentCapability [2]
  **Do:** Create `crates/rho-harness-core/src/agent_mesh/types.rs` defining ALPNs (`rho/agent/task/v1`, `rho/agent/eval/v1`), `AgentCapability` (`ReadOnly`, `Propose`, `Full`), and task payload formats.
  **Covers:** REQ-010, REQ-012.
  **Tests:** Unit tests for capability serialization and permission checks.
  **Verify:** `cargo test -p rho-harness-core agent_mesh::types` -- passes.

  #### Task 3.2: Implement AgentTicket with cryptographic authorization [2]
  **Do:** Create `crates/rho-harness-core/src/agent_mesh/ticket.rs` supporting `rho-agent://` URI scheme encoding `EndpointId`, `RelayUrl`, `AgentCapability`, and HMAC secrets.
  **Covers:** REQ-011, AC-005.
  **Tests:** Unit tests for ticket generation, URI parsing, and round-trip verification.
  **Verify:** `cargo test -p rho-harness-core agent_mesh::ticket` -- passes.

  #### Task 3.3: Implement streaming agent wire framing [3]
  **Do:** Create `crates/rho-harness-core/src/agent_mesh/protocol.rs` handling mutual challenge handshakes, capability verification, and length-delimited JSON frame streaming over Iroh QUIC streams.
  **Covers:** REQ-010, REQ-012, AC-006.
  **Tests:** Unit tests verifying handshake exchange and capability enforcement.
  **Verify:** `cargo test -p rho-harness-core agent_mesh::protocol` -- passes.

- **Slice verification**: `cargo test -p rho-harness-core agent_mesh` and `make crap`.

---

### Slice 4: Remote Agent Peer Execution & CLI Workflows (`rho agent`)

- **Goal**: Connect sovereign nodes for distributed task delegation via `rho agent serve` and `rho agent run`.
- **Acceptance criteria**: Satisfies REQ-013, AC-005.
- **Tasks**:
  #### Task 4.1: Implement AgentWorkerServer [3]
  **Do:** Create `crates/rho-engine/src/agent_mesh/server.rs` running an Iroh endpoint listener. Accepts incoming client connections, validates capabilities, dispatches subagent tasks via `SubagentRunner`, and streams turn events back to the peer.
  **Covers:** REQ-012, REQ-013, AC-005, AC-006.
  **Tests:** Integration test running an in-memory loopback worker server.
  **Verify:** `cargo test -p rho-engine agent_mesh::server` -- passes.

  #### Task 4.2: Implement AgentClientEndpoint [3]
  **Do:** Create `crates/rho-engine/src/agent_mesh/client.rs` to connect to a remote `rho-agent://` ticket, stream task execution requests, and receive streaming output tokens and tool results.
  **Covers:** REQ-013, AC-005.
  **Tests:** Integration test connecting client to worker over loopback.
  **Verify:** `cargo test -p rho-engine agent_mesh::client` -- passes.

  #### Task 4.3: Implement CLI commands `rho agent serve` and `rho agent run` [2]
  **Do:** Add `agent` subcommand in `src/cli/` supporting `serve` (spawns worker, prints ticket) and `run` (connects to ticket, delegates prompt).
  **Covers:** REQ-013, AC-005.
  **Tests:** CLI argument parsing and error handling tests.
  **Verify:** `cargo test -p rho cli::agent` -- passes.

- **Slice verification**: `cargo test --workspace` and `make crap`.

---

### Slice 5: Full Quality Gate, Documentation & Verification

- **Goal**: Comprehensive verification across all crates, doc updates, and CI gate satisfaction.
- **Acceptance criteria**: Satisfies Definition of Done.
- **Tasks**:
  #### Task 5.1: Synchronize documentation and website [2]
  **Do:** Update `README.md`, `docs/`, and `www/` to document the new `lsp` tool, VFS `diff://` / `conflict://` URI schemes, and `rho agent` P2P commands.
  **Covers:** Quality gates.
  **Tests:** Ensure doc links and command examples are valid.
  **Verify:** `cargo test --doc` -- passes.

  #### Task 5.2: Workspace quality verification [2]
  **Do:** Run `make all` to ensure format, clippy, cognitive complexity <= 15 (`cccc`), quality regression (`ripwire`), and CRAP <= 30 pass.
  **Covers:** Definition of Done.
  **Tests:** Entire test suite.
  **Verify:** `make all` -- all checks pass.

---

## Final verification

- `cargo fmt --all -- --check`
- `make clippy`
- `make complexity`
- `make crap`
- `make all`
