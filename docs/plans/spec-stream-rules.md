# Specification: Time-Traveling Stream Rules (TTSR)

**Status**: Draft  
**Target Milestone**: v0.12.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`

---

## 1. Problem Statement
Agents tend to drift or repeat common anti-patterns (e.g. using `Box::leak` in hot loops, suppressing linter errors with `#[allow(...)]`, swallowing errors silently, or modifying generated files). 
- **Prompt Tax**: Putting dozens of negative rules into the system prompt costs valuable tokens on every single turn, diluting attention.
- **Post-Hoc Friction**: Catching violations only after tool execution wastes turns and context rewriting code that should never have been emitted in the first place.

## 2. Proposed Solution: Real-Time Stream Interception
Adopt a reactive rule matching pipeline over the streaming response token channel:
1. Define dormant rules in `.rho/rules.toml` or `~/.config/rho/rules.toml` with regex or syntactic triggers.
2. As the provider streams tokens, an in-flight scanner checks matching tokens.
3. If a forbidden pattern triggers:
   - Immediately abort the provider stream (`stream.abort()`).
   - Inject the triggered rule as a concise system reminder card (`⚠ Injecting rule: <rule-name>`).
   - Promptly re-request completion from the abort point or prompt a targeted course-correction turn.
4. **Sticky Injections**: Injected rules survive context compaction so the model does not repeat the mistake later in the session.

## 3. Implementation Steps
1. Add rule definition schema to `rho-harness-core/src/config/rules.rs`.
2. Integrate in-flight buffer matcher in `rho-engine/src/engine/stream.rs`.
3. Support stream cancellation and injection rewind in `rho-engine/src/engine/runner/turn/`.
