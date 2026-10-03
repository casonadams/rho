# Specification: Decision Model Routing & Gating (`clef-flash`)

**Status**: Draft  
**Target Milestone**: v0.11.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`

---

## 1. Problem Statement
Current LLM agent harnesses rely on heavy autoregressive LLMs for:
- Routing queries (e.g. deciding whether to use a cheap model `smol`, standard model, or slow deep reasoning model `slow`).
- Permission guardrails (evaluating whether a shell command is safe to execute or requires manual approval).
- Reviewing diffs / grading tool outputs.

Autoregressive models suffer from:
1. **High Latency**: 500ms–2000ms just to output a handful of tokens.
2. **Parsing Fragility**: The model might output markdown wrappers, reasoning preambles, or malformed JSON (`{"safe": true}`).
3. **Context Overhead**: Prompting for classification consumes tokens and requires complex prompt engineering to prevent drift.

## 2. The Clef-Flash / System One Model
Cloudflare's `clef-flash` (available on Ollama via the `/v1/systemone` endpoint) is a 9B non-autoregressive decision model.
- **Single Non-Autoregressive Forward Pass**: Given a `state` string and a dictionary of typed `questions` (`choice`, `noul`, `score`), all answers and probability distributions are scored jointly in one pass (~30–80ms locally on Apple Silicon).
- **Structured Typed Guarantees**: Outputs exact enum choices or boolean probabilities (`noul`) without generating free-form text.

## 3. Applications in `rho`

### Application A: Instant Model Tier Routing (`/route`)
When a prompt arrives (e.g. `rho -p "fix typo in readme"` vs `rho -p "rearchitect the auth subsystem"`):
- `state`: User prompt + workspace summary.
- Question `tier`:
  - `type`: `choice`
  - `criteria`:
    - `smol`: Simple typo fixes, single file modifications, formatting, git status inspection.
    - `standard`: Normal feature development, bug investigations, multi-file edits.
    - `slow`: Complex architectural redesigns, concurrency issues, deep algorithmic debugging.
- Result: Returns probability-calibrated tier in <50ms. `rho` selects the matching role model (`--smol`, `--model`, or `--slow`) automatically without user friction.

### Application B: High-Speed Bash Guard Gating
`rho` currently uses `ollama/qwen2.5-coder:7b` autoregressively for command safety checks (`models.guard`).
With `clef-flash`:
- Call `POST /v1/systemone`
- `state`: Proposed shell command + working directory context.
- Question `is_safe`:
  - `type`: `noul`
  - `criteria`:
    - `true`: Read-only commands, compilation, running tests, local status checks.
    - `false`: Destructive file removals (`rm -rf`), force pushes, remote deployments, database wipes, secret reading.
- Result: If `is_safe.noul > 0.95`, command executes instantly. If `< 0.95`, interactive approval modal pops with the calibrated risk confidence.

## 4. Architecture & Interface

```rust
pub struct SystemOneClient {
    endpoint: Url,
    client: reqwest::Client,
}

pub enum SystemOneQuestion {
    Choice { instructions: String, options: HashMap<String, String> },
    Noul { instructions: String, criteria_true: String, criteria_false: String },
    Score { instructions: String, levels: Vec<String> },
}
```

## 5. Implementation Steps
1. Add `rho-engine/src/provider/systemone.rs` client for Ollama's `/v1/systemone` API.
2. Introduce `decision` provider variant in `rho-engine` alongside chat completion providers.
3. Wire `clef-flash` into the permission guard evaluation pipeline in `rho-engine/src/permission/`.
4. Add benchmark comparison tests measuring latency vs autoregressive guard models.
