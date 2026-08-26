# rust-ai

`rust-ai` is a local coding-agent CLI built on Rig 0.42. Rig owns model requests, canonical messages, native tool calling, tool-result correlation, usage normalization, and the finite agent loop. rust-ai supplies provider selection, tools, approval policy, local sessions, terminal rendering, and bounded model-visible context.

## Running

```sh
cargo run -- --provider anthropic --model claude-sonnet-4-6
cargo run -- --provider openai --model gpt-5.4 --prompt "summarize this repository"
cargo run -- --resume SESSION_ID
```

Run `rust-ai --help` for flags and `rust-ai` followed by `/help` for interactive commands. `--max-output-tokens` sets an explicit per-call output limit. When it is omitted, the selected provider's default applies. `--max-turns` sets the finite model-call budget; the current default is 100. Content telemetry is disabled.

## Providers and authentication

Provider identity selects both authentication and Rig transport. There is no unknown-provider fallback. `openai`, `chatgpt`, and `copilot` are distinct identities and are not aliases.

| Provider | Authentication | Environment variable |
| --- | --- | --- |
| `anthropic` | API key | `ANTHROPIC_API_KEY` |
| `openai` | API key | `OPENAI_API_KEY` |
| `deepseek` | API key | `DEEPSEEK_API_KEY` |
| `gemini` (`google` alias) | API key | `GEMINI_API_KEY` |
| `groq` | API key | `GROQ_API_KEY` |
| `openrouter` | API key | `OPENROUTER_API_KEY` |
| `xai` | API key | `XAI_API_KEY` |
| `mistral` | API key | `MISTRAL_API_KEY` |
| `cohere` | API key | `COHERE_API_KEY` |
| `chatgpt` | ChatGPT subscription OAuth through Rig | none |
| `copilot` | GitHub/Copilot subscription OAuth through Rig | none |
| `ollama` | local service; no login | `OLLAMA_HOST` is optional |

API-key providers read the provider-specific environment variable first and then rust-ai's API-key store. Login prompts for a masked key and verifies it through Rig where that provider supports verification; otherwise validation is explicitly deferred.

```sh
rust-ai login openai
rust-ai logout openai
```

ChatGPT and Copilot use Rig's separate subscription OAuth implementations, device authorization, refresh, and provider-specific endpoints. Device flow starts only after explicit interactive login:

```sh
rust-ai login chatgpt
rust-ai login copilot
rust-ai logout chatgpt
rust-ai logout copilot
```

Normal startup, one-shot use, and model construction do not start a device flow. Missing, stale, revoked, or ineligible subscription credentials fail with an authentication error and require an explicit login. OAuth support does not imply that every model or account entitlement is available.

By default, configuration is under `~/.config/rust-ai`; set `RUST_AI_HOME` to replace that directory. Stored API keys use `auth.json`. OAuth tokens remain separate under `tokens/chatgpt/` and `tokens/copilot/`. Logout removes only the selected provider's stored credentials. It cannot remove an API key exported by the shell.

Credentials must not be copied into prompts or tool input. rust-ai redacts known credentials from canonical messages, checkpoints, compaction artifacts, audit events, errors, telemetry, debug output, and terminal rendering. Credential files and token directories use restrictive permissions on Unix. Session files can still contain prompts, source excerpts, tool arguments, and tool output, so treat the entire configuration directory as private and do not publish it.

## Sessions and recovery

Sessions are local, version-2 JSONL files in `sessions/`. Version-1 or unversioned sessions are intentionally incompatible with `--resume` and fail locally rather than being partially replayed. Unknown IDs and malformed sessions also fail without provider fallback.

A successful continuation persists the exact canonical messages returned by Rig only after the complete turn succeeds. Every persisted tool call must have exactly one correlated terminal result. Audit records are stored alongside canonical batches but remain a distinct record type.

- A later prompt in the same REPL continues with prior successful history.
- `--resume SESSION_ID` reopens successful canonical history and any pending budget checkpoint.
- `/clear` creates a fresh v2 session. It does not delete the previous session or its audit history.
- Ctrl+C cancels the active operation, records a terminal cancellation, and does not persist partial assistant content or dangling tool calls. Bash child cleanup remains active.
- A model-call budget exhaustion is not a successful turn. Rig's complete, validated `MaxTurnsError` history is stored as a separate run checkpoint, not appended to canonical memory.
- The next explicit prompt receives a pending checkpoint exactly once. Checkpoint history plus the new Rig messages is promoted atomically only after that continuation succeeds. A failed or cancelled continuation leaves the checkpoint available for another restart or `--resume`.

There is no automatic whole-run replay after failure, which avoids repeating tool side effects.

## Tools and approvals

Read, read-only Bash, web search, and web fetch can run without approval, subject to validation, timeout, output-size, and network protections. Write, edit, and mutating or unproven Bash calls require explicit approval unless `--auto-approve` is set. Enforcement occurs again at the tool execution boundary. A denial performs no mutation and becomes a model-visible tool error so the agent can recover. Tool calls execute sequentially.

Three identical consecutive calls are blocked before the third operation executes. Meaningful arguments are normalized for Bash and web search; a different or interleaved call resets the count.

## Limits, usage, and context

Normalized usage is shown and recorded when the provider reports it. If unavailable, rust-ai reports that it is unavailable rather than estimating tokens from characters. A provider-default output limit is used unless `max_output_tokens`, `AI_MAX_OUTPUT_TOKENS`, or `--max-output-tokens` is set. Every run has a finite `max_turns` budget, configurable in `config.toml`, with `AI_MAX_TURNS`, or with `--max-turns`.

Durable canonical history and audit records are not the same as model-visible context. The full valid v2 history remains on disk and resumable. Windowing and compaction only reduce what is supplied to the model:

- model-visible sliding window: 24 messages
- bounded compaction artifact: 8 KiB
- `AI_CONTEXT_WINDOW_MESSAGES`: positive message-window override
- `AI_COMPACTION_MAX_BYTES`: positive artifact-size override

The corresponding `config.toml` keys are `context_window_messages` and `compaction_max_bytes`. Compaction artifacts are stored beside their session and redact known credentials. Windowing, compaction, `/clear`, and cancellation do not delete previous session files or audit history. There is currently no context-specific tool-result trimming or clearing; complete call/result pairs remain intact.

## Configuration example

```toml
provider = "anthropic"
model = "claude-sonnet-4-6"
auto_approve = false
max_turns = 100
# max_output_tokens = 8192
context_window_messages = 24
compaction_max_bytes = 8192
```

Environment variables override the file, and CLI flags override both where a corresponding flag exists. See [release readiness](docs/release-readiness.md) for the offline smoke checklist and rollback notes.
