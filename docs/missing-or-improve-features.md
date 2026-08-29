# Missing or Improve Features

Comparison of `rho` against `pi.dev` (pi agent harness), to identify gaps and improvement opportunities.

> Note: `rho` is already a superset of pi in several areas (OAuth device-flow login, live subscription
> quota polling, a plugin/extension system, skills, an eval harness, and more default tools). This
> doc focuses on things pi has that rho does **not** (or has only partially).

---

## Confirmed: what rho already has (NOT a gap)

Before chasing pi's features, confirm these already exist in rho:

- **Compaction / auto-summary** — `src/session/context/` implements a real compaction pipeline:
  `CodingCompactor` (rig `Compactor` trait) with FNV-1a deduplication (`hashing.rs`),
  on-disk sidecar persistence (`state.rs`, `context.json`), and critical-fact extraction (`artifact.rs`).
  Configurable via `compaction_max_bytes`.
- **Plugin/extension system** with lifecycle hooks, manifests, external-process discovery (`src/plugin/`).
- **Skills** system loaded from multiple locations (`src/skills/`).
- **Eval harness + offline contract tests** (`src/engine/eval/`).
- **Tool execution policy / approval authority** (`src/tools/approval/`, `src/tools/policy.rs`).
  This is arguably *more* than pi, which explicitly ships no permission system.
- **Session resume** (`rho --resume <id>`).

---

## Genuine gaps (pi has it, rho doesn't)

### 1. Session tree / branching / rewind — HIGHEST VALUE
pi's flagship feature: view the entire conversation as a **tree**, jump to any point, **branch** from a
prior message, and **rewind** to it. Reviews repeatedly call this the standout feature ("something I
miss when using other tools").

rho's current model: linear session history + `--resume <id>` (reopens one session). No visual tree,
no branching, no rewind-to-a-point.

**Why it matters:** sessions are not linear. Users rework decisions, want to fork from an earlier
state, and share reproducible branches. Branching also pairs naturally with the eval/rewind story.

**Deep dive questions:**
- Do we need a visual in-TUI tree, or a CLI/session-browser surface (pi's `/tree` is a CLI command)?
- Session storage currently keyed by a single id — do we model sessions as a DAG of turns instead of a
  flat list? This affects the on-disk format and `--resume` semantics.
- Can we reuse the existing `SessionManager`/sidecar pattern (`context.json`) for tree persistence?

### 2. `/compact` command + session-info view — MEDIUM VALUE

pi exposes `/compact` (summarize old messages) and `/session` (tokens/cost/session info).
rho's compaction engine exists but is **not** reachable from the REPL — there is no `/compact` command,
and no `/session` info view. The footer shows context %, but there's no manual trigger or session
diagnostics.

**Why it matters:** users want visibility into and control over compaction (freeing context, seeing
what was summarized).

**Deep dive questions:**
- Wire a `/compact` command that calls the existing `CodingCompactor` (it already exists and is tested).
- Does `/session` need token/cost tracking, or is context % alone enough for v1?
- Confirm whether compaction triggers automatically when context fills, or only when the user asks.

### 3. `--continue` / last-session-in-cwd — MEDIUM VALUE

pi's `pi --continue` reopens the **last session in the current repo/folder**. rho has `list_sessions()`
(newest-first) but **no** "last session in cwd" logic, so `--resume` still requires an explicit id.

**Why it matters:** "reopen where I was, in this directory" is the common case; typing a session id is
friction.

**Deep dive questions:**
- `list_sessions()` already returns ids sorted newest-first — the remaining work is persisting
  last-used per working directory and resolving it for `--continue`.
- How does it interact with session branching (#1) if we add it? (Which id is "last" — the newest file,
  or the newest branch?)

### 4. Live model picker keyboard shortcut — LOW/MEDIUM VALUE

pi exposes `Ctrl+L` (model selector), `Ctrl+P` (cycle scoped models), `Shift+Tab` (thinking level).
rho already has `/model [model] [provider]` + completion (`src/repl/interactive.rs`, `/help` lists it),
so the **functionality** exists. What is missing is a **keyboard shortcut** — no `Ctrl+L` binding in
`src/repl/`.

**Why it matters:** a shortcut is smoother than typing `/model`, but this is polish, not a capability
gap. Lower priority given the slash command already works.

**Deep dive questions:**
- Confirm whether a keyboard binding is worth the added input-complexity vs. the existing `/model`
  command (which also handles provider switching + listing).
- rho's provider catalog (`src/engine/provider/catalog.rs`) already exposes the model list needed.

### 5. Prompt templates — LOW VALUE, cheap to add

pi ships reusable prompt Markdown files that can be invoked. rho has no prompt-template system.

**Why it matters:** small but genuinely useful for re-running common workflows (summarize repo, review
diff, etc.) without retyping.

**Deep dive questions:**
- Where do templates resolve from? (`~/.config/rho/`, project dir, or built-ins?)
- Invoke via `/command`, a dedicated `rho run <template>` subcommand, or both?

### 6. Package sharing (bundle skills+extensions+themes) — MEDIUM/LOW VALUE
pi lets you bundle skills/extensions/themes as shareable packages (npm/git). rho has skills and
plugins but no formal packaging/share flow.

**Why it matters:** ecosystem growth — users sharing configs is a strong distribution channel for pi.

**Deep dive questions:**
- Is packaging worth the maintenance overhead, or can skills/plugins already be shared via git today?
- Reuse an existing archive format vs. designing a manifest-based package spec.

---

## Explicitly out of scope (don't copy)

- **Permissions / sandboxing** — pi ships *no* built-in permission system (runs as user; you
  containerize it). rho's `ToolExecutionPolicy` + approval flow already covers this and is more.
- **Sub-agents / plan mode / MCP** — pi deliberately omits these. rho already has more; don't trim.

---

## Suggested priority

1. **Session tree / branching / rewind** — biggest differentiator, highest user value.
2. **`/compact` + `/session` view** — expose the existing compaction engine (medium effort, high value).
3. **`--continue` (last session in cwd)** — small, high-frequency convenience.
4. **Live model picker keyboard shortcut** — polish; confirm the slash command is enough first.
5. **Prompt templates** — cheap, nice-to-have.
6. **Package sharing** — ecosystem play, deferring is fine.
