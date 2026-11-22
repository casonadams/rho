# Handoff: prompt-cache and token work

State: the prune-decision logging below is **uncommitted** in the working tree (7 files under `crates/rho-engine/src/engine/`: `metrics/{mod,tests,tracker,types}.rs`, `runner/turn/prepare.rs`, `runner/turn/prune/{cache,tests}.rs`). Everything before it is pushed to `origin/main` (`4de2a28c`). `make all` passes (2397 tests) with the uncommitted changes. Commit them first, e.g. `feat(metrics): record prune decisions in run summaries`.

## What changed

1. **`cache_retention` setting** (`auto` default, `short`, `long`).
   - `auto` = 1h Anthropic cache for `claude` subscription logins, 5m for `anthropic` API keys.
   - Subagents always use 5m under `auto`.
   - Set with `rho config cache_retention <value>`.
2. **Cache-aware pruning** (`crates/rho-engine/src/engine/runner/turn/prune/cache.rs`). While the previous request's cache is alive, the same stubs are resent. Pruning advances only if it saves at least 25% of the history it would re-cache (`WARM_REBUILD_RATIO`).
3. **Run summaries** record `model` and per-request `started_ms`.
4. **Shorter tool text** (see below). Bash footer: `[exit 0; N lines, SIZE; log: PATH]`.
5. **Refactors**: `TypedTool`, shared `fd`/`rg` search code, no tuple-packed parameters, one exhaustive config-key match, shared test fixtures.

New message formats the model sees:

| Case | Text |
|---|---|
| failed command | `[exit N]` |
| timeout | `[timeout Ns]` |
| timeout moved to background | `[timeout Ns; background job ID; its output arrives as a follow-up, do not poll]` |
| pruned result | `[tool 'target': pruned N lines, SIZE]` |
| pruned bash | `[bash 'cmd': exit N, pruned N lines, SIZE; log: PATH]` |

The pruner still reads the old wording from saved sessions.

## Install

```
cargo install --path .
```

Restart rho afterwards; a running session keeps the old binary.

## Test 1: does the 1h cache work?

Not yet verified. All idle gaps so far were under 3 minutes.

1. Start a session, do some real work (about 100k+ tokens of history).
2. Wait 15-55 minutes idle.
3. Send one prompt.
4. Run `python3 /tmp/cachecheck.py | tail`.
   - Pass: gap over 5m, first-request `cached` in the hundreds of thousands, `written` in the low thousands.
   - Fail: `cached` about 19939, `written` 130k or more (the 5-minute behavior).

`/tmp/cachecheck.py` and `/tmp/cachegap.py` live in `/tmp`; copy them out if the machine restarts. `cachegap.py` compares cold-start rates by gap length across all sessions.

## Test 2: do the shorter messages behave?

- Run a bash command with 30+ lines of output. Expect the footer `[exit 0; ...; log: PATH]` and that the log path exists.
- Run `sh -c 'exit 3'`. Expect `[exit 3]`.
- Run `sleep 40` with a 5s timeout. Expect `[timeout 5s; background job ...]`, then a follow-up notice when it ends.
- After a few turns, check that old long results show as `[read 'x': pruned N lines, SIZE]`.

## Prune-decision logging (new, uncommitted)

Each run summary now has an optional `prune` field: `{action, saved_tokens, rebuilt_tokens}`.

- `action`: `cold` (cache expired, pruned from scratch), `unchanged` (warm, nothing differs), `kept` (warm, old stubs resent), `rewritten` (warm, re-pruned; the 25% rule fired).
- `saved_tokens` / `rebuilt_tokens` are the two numbers the `WARM_REBUILD_RATIO` rule compared. Both are 0 for `cold` and `unchanged`.
- Pruning runs once per prompt, so there is one decision per run, not per request.
- Recorded in `prepare.rs` via `RunTracker::pruned`, after `RunTracker::start` (which resets state).

The installed binary (`~/.cargo/bin/rho`, 16:57) includes it, but the session that was open then does not. Start a fresh session.

## Test 3: which prune decision precedes a cache loss?

1. Use rho in a new session for a while, with several prompts and some short gaps.
2. Run `python3 /tmp/prunecheck.py | tail -30` (prints gap, action, saved, rebuilt, first-request cached/written, requests; it covers the 6 newest sessions).
3. Look for runs with a short gap but `cached` about 19.9k and large `written`.
   - If these are `rewritten`, the 25% rule is too eager: raise `WARM_REBUILD_RATIO` or weigh the remaining requests.
   - If they are `kept`/`unchanged`, pruning is not the cause; look elsewhere (system prompt, tool list or preamble changes).
4. If `prune` never appears, check `mise which rho`: the mise shim may launch a different binary than `~/.cargo/bin/rho`.

`/tmp/prunecheck.py` is also in `/tmp` only; copy it out if the machine restarts.

## Open question

On 2026-11-21 at 23:29 a run with a 0.8m idle gap lost its whole cache (cached 19.9k, written 291k). Likely cause: the 25% rewrite rule in the pruner decided to re-prune after a 141-request run. This was a guess until Test 3 gives data.

Next step after Test 3: tune `WARM_REBUILD_RATIO` from real data. The rule also ignores how many requests remain in the run. `docs/ui.md` mentions pruning; check whether it needs a note on the new field.

## Not done

- Read tool's truncation notice is still long: `[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]`
- 401 refresh-and-retry code is still duplicated across Claude, ChatGPT and Antigravity (error text differs and is tested).
- Two authorize-URL builders (`auth/pkce.rs`, `mcp/oauth/login.rs`) are not merged; their behavior differs.
- Full test suite takes about 17s against a 15s repo target.
