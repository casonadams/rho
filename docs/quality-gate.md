# In-Process Static Quality Gate

`rho` includes a native, in-process static code quality verification gate powered by [`ripwire-rs`](https://crates.io/crates/ripwire-rs).

Whenever an agent attempts to conclude its work, `rho` deterministically evaluates the working tree changes before accepting completion. If the changes introduce code complexity regressions, broken function contracts, or untested blast radius, `rho` rejects the completion and sends the agent back to work with actionable, line-accurate feedback.

---

## Why an In-Process Static Gate?

Traditional coding agents rely on **LLM self-evaluation** ("Did you finish the task correctly?") or prompt assertions. This has well-known failure modes:

1. **Confirmation Bias**: The model hallucinates that its implementation is sound even when it broke caller signatures or introduced massive single-function bloat.
2. **Slow / Expensive Feedback Loops**: Running full compilation suites (`cargo check`, `npm test`) on every turn introduces significant latency and token costs.
3. **Silent Deterioration**: Code slowly accrues cyclomatic complexity, deeply nested branches, and untested pathways that future agent runs struggle to navigate.

`rho` solves this by embedding `ripwire-rs` directly as an in-memory library. Verification executes in **milliseconds** without spawning shell processes, querying tree-sitter ASTs, PageRank call graphs, and git deltas in-process.

---

## What the Quality Gate Verifies

When an agent proposes completion (emitting text without further tool calls), `rho` runs four static checks:

### 1. Code Quality Regressions (`evaluate_quality_delta`)
Evaluates the delta between the working tree and git `HEAD` (or a pinned `.ripwire-rs_quality_baseline`):
- **Cognitive Complexity**: Flags any function exceeding threshold ($\le 15$, SonarSource Campbell standard).
- **Nesting Depth**: Flags deeply nested conditional logic ($> 4$ levels).
- **Parameter Count**: Prevents signature bloat ($> 5$ parameters).
- **Function Length (LOC)**: Flags monolithic, oversized functions.
- **AST Duplication / Clones**: Detects copy-pasted blocks across the repository.
- **Dead Code**: Detects new unreferenced private functions or helpers.
- **Defect Shapes**: Catches dangerous patterns like format-string arity mismatches and unsafe slicing.

### 2. Broken Public Contracts (`check_symbol_contract`)
When an agent edits or refactors an existing function, method, or type:
- Compares the new signature against the baseline git `HEAD` contract.
- Scans all 1-hop callers across the workspace to verify parameter count and type compatibility.
- Flags exact broken callsites (e.g. `called with 2 arguments, requires 3`).

### 3. Untested Blast Radius (`find_affected_tests`)
- Traces the transitive call graph from modified symbols to existing unit and integration tests.
- Flags impacted symbols with **zero** test reach.
- Identifies candidate test files that exercise the touched modules.

### 4. Zero-Overhead Read-Only Bypass
If the agent only answered questions or inspected code without modifying files, `working_tree_changes` returns empty in $\sim 0.5\text{ms}$ and completion is accepted immediately without delay.

---

## The Push-Back Loop

If the gate detects blocking issues, it intercepts completion and loops the agent back into execution:

```text
Agent Turn N: "I have finished refactoring the parser." (0 tool calls)
        │
        ▼
   [Quality Gate Intercept]
   - Cognitive complexity in parser.rs:84 rose from 10 to 22 (> 15).
   - Incompatible caller in runner.rs:120 (missing argument).
        │
        ▼
Agent Turn N+1: Injects structured rejection prompt:
   "[Static Quality Gate Rejection - Action Required]
    • Cognitive Complexity: `parse_tokens` in `crates/rho-engine/src/parser.rs:84`
      Score was 10, now 22 (threshold is <= 15).
      Action: Invert conditionals or extract private helpers.
    [Quality Gate Attempt 1 of 3]"
        │
        ▼
Agent uses `edit` to refactor helper functions and fix callsites.
        │
        ▼
Agent Turn N+2: "Refactored into parse_token_inner, complexity is now 8."
        │
        ▼
   [Quality Gate Intercept] -> Passed! -> Task Completed.
```

The loop supports up to **3 attempts** (`max_quality_gate_retries = 3`). If issues remain after 3 cycles, completion is allowed to proceed to avoid infinite loops on intractable legacy codebases.

---

## Configuration

The static quality gate is enabled by default for all git repositories.

Configure it in `~/.config/rho/config.toml` or project `.rho/config.toml`:

```toml
# Enable or disable the in-process static quality gate (default: true)
quality_gate = true
```

---

## Under the Hood: `ripwire-rs`

The quality gate is powered by [`ripwire-rs`](https://crates.io/crates/ripwire-rs), an open-source static analysis engine and context router for AI coding agents. 

Key attributes of the library:
- **Sub-millisecond AST queries**: Built on Tree-sitter grammars (Rust, TypeScript, JavaScript, Python, Go, C/C++, JSON).
- **In-process Call Graph**: Compressed Sparse Row (CSR) matrix representation with PageRank symbol centrality.
- **Direct Git Integration**: Uses native `gix` for high-throughput working tree inspection without spawning `git` CLI processes.
