# rho — Architecture Findings & Improvement Notes

Compiled 2026-09-11. This is an informal diagnosis of rho's design, cross-referenced against the coding-agent architecture literature. Not a spec; a place to capture the research and surface-level findings so they aren't lost.

---

## Part 1 — Is rho's design good?

### What the research says matters

The single most-cited finding in this space comes from the VILA-Lab *"Dive into Claude Code"* paper (arXiv:2604.14228), which reverse-engineered Claude Code's leaked ~512K-line TypeScript codebase:

- Only **~1.6% of the codebase is AI/decision logic; 98.4% is operational infrastructure** — permission gates, tool routing, context compaction, recovery logic, session persistence. The model reasons; the harness does everything else.
- The **harness effect**: the same model in a different harness scores 16+ points higher/lower on identical benchmarks. Harness engineering dominates model choice.

These are the two facts that matter. rho is architected around both of them.

### rho nails the things that matter

- **Domain/infra separation.** `rho-harness-core` is deliberately framework- and LLM-free. This mirrors Rig's own `rig-core` / `rig-agent` split (provider-neutral abstractions vs. the orchestration state machine). Value: the hard-to-change rules (config, sessions, tokens, presentation contract) stay isolated and unit-testable; the volatile parts (provider wire formats, Rig) live at the edges.
- **Permission as architecture, not config.** rho's layered gate (baseline allowlist → bash lexer/path analysis → policy evaluation → optional external plugin → interactive/hard-fail prompt) maps onto Claude Code's "seven safety layers" and the paper's classification of permission systems as a core component.
- **Context compaction as first-class infra.** The `compactor/` (overflow-triggered summarization, compaction nodes projected into the session tree, durable history append-only) matches the documented compaction-pipeline pattern.
- **Three-loop decomposition.** Splitting TUI events / agent turn / tool execution — and decoupling the two via a `coordinator/` — is the right mental model. UI frame batching never blocks turn execution.

### The risk that isn't structural

**Rig is the real exposure.** Rig is pre-1.0 with an explicit *"Here be dragons — breaking changes"* warning and roughly monthly releases (v0.40 early Jul, 0.41 Jul, 0.42 Aug 2026). The mitigations are real but they're *containment*, not removal:

- Rig usage is confined to `rho-engine` and the CLI. `rho-harness-core` never imports Rig, so a breaking change should stay in one layer.
- This is exactly where the layering decision pays off — it bought rho the resilience to absorb Rig churn. But fast-moving pre-1.0 dependency churn is still a maintenance burden to watch.

**Bottom line:** structurally this is good design — it implements the lessons the field has converged on rather than reinventing them. The main thing to monitor is Rig's dependency churn.

---

## Part 2 — Complexity surface: what to actually reduce

"Complexity surface" is three distinct problems with different fixes. Don't treat them as one.

### Measurements (engine subsystems, real, 2026-09-11)

| Subsystem | LOC | Files | test : src | Read |
|-----------|-----|-------|------------|------|
| `tools/`          | ~7,000 | 70 | 1:0.4 | biggest code sink |
| `engine/`         | ~6,400 | 64 | 1:0.6 | healthy, well-tested |
| `permission/`     | ~2,000 | 29 | 1:1.7 | excellent isolation |
| `auth/`           | ~2,300 | 26 | 1:0.14 | — |
| `antigravity/`    | ~2,000 | 21 | 1:0.61 | — |
| `plugin/`         | ~1,970 | 19 | 1:0.58 | **duplicate runtime** |
| `mcp/`            | ~1,970 | 17 | 1:0.10 | — |
| `provider/`       | ~1,470 | 12 | 1:0.31 | — |
| `claude/`         | ~950  | 10 | 1:0.42 | — |
| `ollama/` `chatgpt/` `process/` | small | — | ~1:1 | thin wrappers |

### The finding: `plugin/` is the problem, not the size

`permission/` and `engine/` are **healthy** — high test ratios, high cohesion, matches the "permissions as architecture" best practice. Fragmenting them would *hurt* the exact thing the "98.4% infra" lesson values. Don't touch them.

`plugin/` has a real structural smell: **a second protocol stack is being reimplemented inside the engine**, parallel to the turn loop. Concrete evidence:

- `daemon/process.rs` — hand-rolled JSON-RPC over stdio: id counter, pending-response `HashMap`, `mpsc` + `oneshot` channels, 600s timeout, `kill_on_drop`, process-group guard.
- `host/dispatcher.rs` — an 18-line `match` dispatching 8 methods, backed by 24 `HostUi*` param types in `host/types.rs`.
- A **third** copy of the protocol in `protocol/`.

The engine is now running its own daemon supervision + protocol + host callbacks — a whole second communication domain. That's the definition of an unbounded surface: a subsystem the model can't reason about from the outside.

### Fix 1 — Stop reimplementing plugins (high leverage) [COMPLETED 2026-09-12]

**Executed:** Converged all external capabilities onto standard MCP (`~/.agents/mcp.json` / `.mcp.json`) and replaced the proprietary JSON-RPC daemon runtime with lightweight, one-shot process hooks (`.rho/hooks/` / `crates/rho-engine/src/hook/`). Completely deleted `crates/rho-plugin-sdk`, `crates/rho-engine/src/plugin/` (daemon, protocol, host UI), and `src/cli/plugin/` (~2,000 LOC eliminated). `config.toml` simplified to pure agent preferences.

### Fix 2 — Document the extensibility seam (cheap, free)

Mature CLAs partition their extension surface by graduated risk (hooks = zero context cost → MCP = high). rho has these layers but they're implicit. Add a small doc or `pub mod` grouping stating the intended seams: what a plugin may touch, what MCP exposes, what hooks can see. Documenting a seam prevents the next feature from crossing it — pure risk reduction.

### Fix 3 — Cap `web/` growth with a seam (proactive)

`web/` is the largest subsystem and growing (search/result, fetch/html extract). Enforce an emerging convention rather than inventing one: keep query/parse in the query module, rendering/extract in a dedicated `extract/` module, and stop individual tools ballooning past ~200 lines. The pattern already exists (each tool has its own `query.rs`).

### What NOT to do

- Don't slice `permission/` or `engine/` smaller — their test ratios and cohesion are already excellent.
- Don't merge the three plugin copies by renaming — that's churn without structural gain. The gain is the copy not needing to exist.

---

## Sources

- VILA-Lab, *Dive into Claude Code: The Design Space of Today's and Future AI Agent Systems* (arXiv:2604.14228) — the 1.6% / 98.4% finding and design-space framework.
- Hysen Labs, *Rig LLM Framework Review* (hysenlabs.com) — rig-core/rig-agent split, pre-1.0 churn, provider flexibility vs. migration cost.
- OpenAI, *Unrolling the Codex agent loop* — harness intelligence lives in context assembly, permission gating, and tool dispatch.
- rho engine source, `crates/rho-engine/src/` — subsystem measurements above.
- rho `ARCHITECTURE.md`, `AGENTS.md`, `README.md` — crate boundaries, conventions, testing policy.
