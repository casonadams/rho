# Roadmap: Artificial Symbiotic Intelligence in Rho

This document establishes the strategic vision, architectural milestones, and phased roadmap to transform `rho` from a single-operator coding CLI into a decentralized, peer-to-peer orchestration harness for Artificial Symbiotic Intelligence (ASI), powered by [Iroh](https://iroh.computer).

Inspired by Bratton, Agüera y Arcas, and Manyika's DeepMind essay (*Artificial Symbiotic Intelligence: Agents, AGI and the Orchestration of Many Minds*, September 2026), `rho` embraces the principle that intelligence at scale is inherently social, plural, and institutional. Collective intelligence emerges not from the weights of a single monolithic model, but from the rules, scaffolds, verification protocols, and cooperative friction among heterogeneous models, tools, and humans.

---

## 1. Architectural Principles

1. **Decomposable Agency**: An agent is not a singular digital twin; it is a temporary, recombinant assemblage of model weights, persona guidelines, bounded tools, and scoped context windows. Agency is decoupled from intent.
2. **The Harness as Institution**: In accordance with the DeepMind thesis, collective capability resides in the embedded procedures, invariants, and feedback mechanisms of the scaffold. The harness enforces separation of powers, capability checks, and verification gates.
3. **Decentralized Sovereign Peering via Iroh**: Agents communicate across processes, containers, and remote nodes without centralized cloud brokers, databases, or tracking telemetry. Every node is identified by an Ed25519 `EndpointId` over end-to-end encrypted QUIC with automatic NAT traversal.
4. **Adversarial Triads over Monocultures**: Hard problems require dialectical tension. Proposers, Critics, and Arbiters execute on distinct model families to eliminate confirmation bias, hallucination, and attention rot.
5. **Polyphonic Human Direction**: The human operator does not micromanage low-level syntax in a 1:1 chat loop; they operate as the high-level intent, steering, and moral arbiter layer overseeing parallel cognitive streams.

---

## 2. Milestone Overview

```
+-------------------------------------------------------------------------------+
|  Phase 1: Subagent Decomposable Agency (In-Harness Delegation)                |
|  - Role-based specialization (Scout, Critic, Planner, Worker)                 |
|  - In-process isolated context forks & child turn execution                   |
|  - Native `subagent` delegation tool & structured result aggregation          |
+-------------------------------------------------------------------------------+
                                      |
                                      v
+-------------------------------------------------------------------------------+
|  Phase 2: Sovereign Agent Endpoints over Iroh (P2P QUIC Mesh)                 |
|  - Dedicated Iroh ALPNs (`rho/agent/v1`, `rho/agent/eval/v1`)                 |
|  - Cryptographic capability tickets (read-only, proposal, mutation)           |
|  - Distributed discovery, cross-machine task distribution, zero telemetry     |
+-------------------------------------------------------------------------------+
                                      |
                                      v
+-------------------------------------------------------------------------------+
|  Phase 3: Dialectical Councils & Institutional Governance                     |
|  - Adversarial Triad: Proposer + Critic/Skeptic + Arbiter                    |
|  - Deterministic institutional checks: test gating, CRAP <= 30 enforcement    |
|  - Multi-agent consensus voting & BLAKE3 artifact content-addressing          |
+-------------------------------------------------------------------------------+
                                      |
                                      v
+-------------------------------------------------------------------------------+
|  Phase 4: Polyphonic Nodal Orchestration TUI                                  |
|  - Multi-stream terminal dashboard & nodal state visualizations               |
|  - Real-time parallel reasoning tracks & dynamic steering                     |
|  - Intent-level orchestration controls for the cognitive crossover era        |
+-------------------------------------------------------------------------------+
```

---

## 3. Milestone Details

### Phase 1: Subagent Decomposable Agency (In-Harness Delegation)

* **Objective**: Break the linear single-agent prompt loop within `rho` by introducing first-class subagent task delegation with isolated context lifecycles and role specialization.
* **Core Concepts**:
  * **Role Profiles**: Define declarative agent roles with tailored toolsets, temperatures, model assignments, and system prompts:
    * `Scout`: Read-only file discovery (`fd`, `rg`, `read`) with compact context limits for structural exploration.
    * `Worker`: Execution agent with file mutation and test runner access.
    * `Critic`: Adversarial reviewer with diff analysis tools, penalizing regressions and missing invariants.
    * `Planner`: High-level decomposing agent generating structured task trees without file mutation.
  * **`subagent` Tool**: Allows the lead agent to spawn bounded subtasks with dedicated instructions, receiving structured summaries without polluting the root context window.
  * **Context Confinement**: Child agent executions run in isolated branches. Their intermediate tool thrash and token consumption are discarded upon completion, returning only high-density synthesis to the parent.
* **Success Criteria**:
  * Root agent can spawn a background `Scout` to explore a repository sub-tree while parent plans next steps.
  * Context growth is bounded by summarizing subagent outcomes into parent history.
  * 100% test coverage with zero CRAP regressions on existing engine modules.

---

### Phase 2: Sovereign Agent Endpoints over Iroh (P2P QUIC Mesh)

* **Objective**: Promote agents from in-process tasks to sovereign network endpoints communicating peer-to-peer over Iroh QUIC streams.
* **Core Concepts**:
  * **Agent ALPNs**: Standardize Application-Layer Protocol Negotiation identifiers:
    * `rho/agent/task/v1`: Task delegation and streaming execution.
    * `rho/agent/eval/v1`: Evaluation and code critique requests.
    * `rho/agent/sync/v1`: Content-addressed artifact exchange.
  * **Capability-Based Tickets**: Extend `CollabSecret` to agent tickets:
    * Capability levels: `ReadOnly` (scouting), `Propose` (can suggest diffs but not execute shell), `Full` (sandboxed execution).
    * Revocable per-session authentication with Ed25519 public keys.
  * **Cross-Host Agent Peering**: Spin up worker nodes on secondary machines (e.g., high-compute GPU instances running local LLMs or clean Linux sandboxes) and pair them instantly with `rho agent join <ticket>`.
* **Success Criteria**:
  * A local `rho` instance running on macOS can delegate a heavy compilation or evaluation task to a remote Linux worker over an encrypted Iroh QUIC stream behind NAT.
  * Disconnection and reconnection are handled gracefully without aborting parent coordination.
  * Zero centralized infrastructure or tracking dependencies.

---

### Phase 3: Dialectical Councils & Institutional Governance

* **Objective**: Implement institutional decision-making scaffolds where agent teams collectively debate, test, and verify solutions before applying mutations.
* **Core Concepts**:
  * **Adversarial Triad**:
    * *Proposer*: Generates multiple candidate solutions or architectural proposals.
    * *Adversary / Critic*: Strictly tasked with finding bugs, edge cases, missing tests, and security risks.
    * *Arbiter*: Coordinates rounds of debate, runs deterministic validation (`make clippy`, `make test`, `cargo test`, CRAP analysis), and selects the winning approach.
  * **Consensus Rules**:
    * Destructive mutations or pull requests require cryptographic consensus (e.g. 2-of-3 agent approvals + clean test run).
  * **BLAKE3 Content Addressing**:
    * Intermediate code diffs, logs, and artifacts are addressed by hash, enabling zero-copy deduplication and verifiable provenance.
* **Success Criteria**:
  * Complex bug fixes are solved with measurably higher first-pass accuracy and lower regression rates than a solitary agent.
  * Adversarial critique catches injected subtle defects before code lands in the working directory.

---

### Phase 4: Polyphonic Nodal Orchestration TUI

* **Objective**: Evolve `rho`'s terminal user interface from a single-conversation chat into a rich, polyphonic multi-agent command center.
* **Core Concepts**:
  * **Multi-Track Event Streams**: View parallel agent streams in real-time with collapsible tracks and activity badges.
  * **Nodal Visualizations**: Render live agent topology diagrams using Mermaid or ANSI-box layouts in the terminal, showing which agents are querying, deliberating, or evaluating.
  * **Interactive Human Steering**: Allow the human operator to inject guidance, pause an unpromising branch, or redirect the council at the intent level without interrupting running child threads.
* **Success Criteria**:
  * The operator can monitor 3+ simultaneous agent tasks without visual confusion or screen flicker.
  * Subagent errors, critiques, and consensus states are immediately legible in the terminal.
