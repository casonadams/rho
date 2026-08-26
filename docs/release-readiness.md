# Release readiness

This checklist is the release gate for the Rig 0.42 agent-core migration. Run it without API keys, OAuth tokens, provider network access, or paid requests.

## Rollback boundary

Create an annotated release tag at the verified Slice 8 commit before distribution. Rollback means reverting the application to that known commit or to the pre-migration release; it does not mean converting session data.

Version-2 session files are intentionally unreadable by pre-migration builds. After rolling back to a build without v2 support, start a fresh session and leave existing JSONL, audit, checkpoint, and compaction files untouched. Do not rewrite OAuth tokens into the API-key store. Provider-specific Rig token directories may remain on disk for a later compatible build.

Stop release or rollback if any tool call/result pair is incomplete, a checkpoint is appended as successful history before continuation succeeds, a credential sentinel appears on a protected surface, an unknown provider reaches network code, or content telemetry is enabled.

## Automated offline checks

From a clean checkout:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

The suite must exercise these structural behaviors with Rig test models, temporary directories, and fake authorization boundaries:

- every API-key provider model constructs offline and maps to its own Rig identity;
- `openai`, `chatgpt`, and `copilot` remain distinct;
- mocked ChatGPT and Copilot login dispatch selects the requested OAuth provider;
- noninteractive OAuth reload does not start device authorization;
- denied write, edit, and mutating Bash calls have no side effects;
- sequential native tool calls retain complete call/result correlation;
- two-turn continuation and v2 `--resume` provide canonical history once;
- cancellation leaves no partial canonical assistant message or dangling tool call;
- runtime IntentSpecs persist separately from session v2, reject credential sentinels, and expose only the selected bounded intent to the model;
- unfinished-intent recovery continues immediately, Mark complete records explicit user acceptance, Not now changes no durable state, and Abandon archives without deleting history;
- approval selectors require Enter and Deny accepts optional blank feedback without executing the operation;
- model-call budget exhaustion sends no extra request;
- complete budget history becomes a separate checkpoint, survives reopen, and is promoted exactly once only after a successful continuation;
- a failed checkpoint continuation leaves the checkpoint available;
- Slice 6 core and session evaluation reports remain deterministic;
- Slice 7 windowing reduces long-session model-visible bytes while durable history remains valid and resumable;
- compaction remains bounded and restart-safe;
- the third identical consecutive tool call is blocked before execution;
- usage-unavailable reporting does not invent token counts;
- content telemetry remains disabled; and
- credential sentinels are absent from canonical messages, checkpoints, compaction artifacts, audits, metrics, errors, debug text, and rendered output.

## Static inspection

```sh
rg -n "chat/completions|eventsource|extract_tool_calls|OpenAiToolCallAccumulator" src
rg -n "#!\[(allow|expect)|#\[(allow|expect)\(clippy" src
cargo tree -d
```

The first two searches should return no obsolete custom provider HTTP/SSE or fallback parser and no lint suppressions. Review direct dependencies against source usage; duplicate transitive dependencies alone are not grounds for unrelated upgrades.

Inspect representative temporary v2 fixtures produced by tests. They must have a version header, ordered canonical and audit records, complete tool pairs, terminal cancellation records where applicable, and no secret sentinel. A pending run checkpoint must remain a separate record from canonical history.

## Credential-free runtime smoke

Use only deterministic tests for release automation. Do not perform a live OAuth flow or provider completion as part of this checklist. Optional live smoke testing requires an operator's explicit authorization and must not capture tokens, prompts, or model output.

Before publishing, compare `rust-ai --help`, interactive `/help`, and [the README](../README.md). Confirm provider names, auth modes, mutation approvals, provider-default output limits, finite model-call budgets, v2 resume behavior, budget checkpoints, and the 24-message/8192-byte context defaults agree.
