# Configuration, Providers & Skills

This guide covers provider authentication, custom models, system instructions,
and declarative skills in `rho`.

---

## Providers & Authentication

`rho` supports 13+ AI providers via subscription OAuth, API keys, and local
inference:

| Provider       | Auth Type          | Configuration / Login                             |
| :------------- | :----------------- | :------------------------------------------------ |
| `chatgpt`      | Subscription OAuth | `rho login chatgpt` (OAuth PKCE)                  |
| `antigravity`  | Google OAuth       | `rho login antigravity` (OAuth PKCE)              |
| `claude`       | Subscription OAuth | `rho login claude` (OAuth PKCE)                   |
| `openrouter`   | OAuth or API Key   | `OPENROUTER_API_KEY` or `rho login openrouter`    |
| `anthropic`    | API Key            | `ANTHROPIC_API_KEY` or `rho login anthropic`      |
| `openai`       | API Key            | `OPENAI_API_KEY` or `rho login openai`            |
| `deepseek`     | API Key            | `DEEPSEEK_API_KEY` or `rho login deepseek`        |
| `gemini`       | API Key            | `GEMINI_API_KEY` or `rho login gemini`            |
| `groq`         | API Key            | `GROQ_API_KEY` or `rho login groq`                |
| `xai`          | API Key            | `XAI_API_KEY` or `rho login xai`                  |
| `mistral`      | API Key            | `MISTRAL_API_KEY` or `rho login mistral`          |
| `cohere`       | API Key            | `COHERE_API_KEY` or `rho login cohere`            |
| `ollama-cloud` | API Key            | `OLLAMA_API_KEY` or `rho login ollama-cloud`      |
| `local`        | Local Daemon       | `OLLAMA_HOST` (default: `http://127.0.0.1:11434`) |

`rho login` without a provider lists the same providers as `/login`, custom
providers included, with each one's login mode, signed-in accounts, and a `✓`
when it already has credentials.

`OLLAMA_HOST` follows Ollama's own rules: a bare `host` or `host:port` uses
plain HTTP and port 11434, and `0.0.0.0` connects through loopback.

### Multiple Accounts

You can stay logged in to several accounts of one subscription provider, such
as a personal and a work Claude plan. Run `rho login claude` again and sign in
with the other account: the first login stays the default `claude`, and the new
one is saved as `claude:<email>`. Logging in again with an account you already
added refreshes that login instead of adding a duplicate.

- Select a model from a specific account with `claude:me@work.com/<model>` in
  `/model`, `--model`, or `[models]`; `claude/<model>` uses the default login.
- The model selector lists every model once per account, and the login
  selector shows the email of each signed-in account.
- The footer names the account in use (long emails are shortened, keeping the
  domain), and quota is tracked per account.
- `rho logout claude:me@work.com` removes one account.

### Model Lists

rho has no built-in model list. Each session it fetches, in the background and
in parallel, the live model list of every provider you can reach: local Ollama,
every provider you are logged in to, and every custom endpoint. New models show
up without updating rho. Nothing is written to disk.

- The model selector (`/model`, `Ctrl+L`) and `/model` completions fill in as
  each list arrives. Providers still loading or that failed to list are named
  at the top of the selector.
- Both insert the listing's full `<provider>/<model>` spec, so a model keeps the
  provider that listed it. Typing a bare id keeps the provider you are already
  using when its live list carries that model, which matters whenever a model
  id is not unique across providers: Ollama Cloud's `deepseek-v4.1-flash` and
  local Ollama's `gpt-oss:20b` stay on the provider serving them, instead of
  being routed to `deepseek` or `openai` by the model's name.
- `rho models` fetches the active provider's list and exits with an error if
  the provider cannot be reached.
- Context windows come from the same lists where the provider publishes them
  (Anthropic and Claude login: `max_input_tokens`; ChatGPT login:
  `context_window`; Gemini: `inputTokenLimit`; OpenRouter, Groq, DeepSeek,
  Mistral, xAI, Cohere: their documented fields; local Ollama: `/api/show` and
  `/api/ps`; Ollama Cloud: `/api/show` on ollama.com with your key).
- Google Antigravity has no public model-list API, so rho merges its live
  runtime catalog with a small fallback list.
- Offline, you can still switch with `/model <provider>/<model>`.

rho also has no built-in default model. Until you pick one with `/model`
(`Ctrl+S` in the selector saves it as the default), no model is selected and
prompts are refused;
`rho -p` and other headless runs exit with an error asking for `--model` or
`rho config model <provider>/<model>`. `--provider X` without `--model` uses
`[models].X` from your config if set, otherwise leaves the model unselected.

Credentials and tokens are stored securely in `~/.config/rho/auth.json` (or
`$RHO_HOME/auth.json`).

> **Privacy Note**: `rho` collects no telemetry and stores all credentials and
> session data strictly on your local machine. API requests are dispatched
> directly to your configured provider endpoints with no intermediary servers.

### Custom OpenAI-Compatible Endpoints

Connect any OpenAI-compatible provider at runtime without rebuilding. Add an
entry to `~/.config/rho/config.toml` (providers are not read from project files):

```toml
[providers.custom_endpoint]
base_url = "https://api.example.com/v1"
key_env = "CUSTOM_API_KEY" # optional; falls back to `rho login custom_endpoint`
```

Select your custom provider using:

```sh
rho --model custom_endpoint/my-model-id
```

Or switch in the REPL with `/model custom_endpoint/my-model-id`.

> **Security Note**: `base_url` requires `http` or `https`. Requests to private
> or loopback IP ranges are rejected unless explicitly allowed with
> `allow_private_network = true` in `config.toml` (or
> `WEB_ALLOW_PRIVATE_NETWORK=true`).

---

## Configuration Hierarchy

`rho` loads configuration by merging settings in order of precedence (highest to
lowest):

1. **CLI Flags** (`--model <provider>/<model>`, `--provider` (optional/legacy), `--thinking`, `--no-permission`,
   etc.)
2. **Environment Variables** (`RHO_MODEL`, `RHO_PROVIDER`, `AI_MODEL`, `AI_PROVIDER`, `AI_THINKING_LEVEL`,
   `AI_MAX_OUTPUT_TOKENS`, `AI_MAX_TURNS`, `AI_CONTEXT_WINDOW_MESSAGES`,
   `AI_COMPACTION_MAX_BYTES`, `RHO_HOME`)
3. **Project Configuration** (`.rho/config.toml`): models, thinking level, token and context limits, search and
   fetch settings, queue modes, session retention, UI, tool settings (`[tools]`), and subagent model pins.
   `permission`, `providers`, `allow_private_network`, `[worktree]`, and MCP servers are read from the global file
   alone, so a cloned repository cannot change them.
4. **Global Configuration** (`~/.config/rho/config.toml`)

```text
~/.agents/
├── AGENTS.md       # Global instructions & engineering defaults
├── hooks/          # Global lifecycle hooks (fallback)
├── mcp.json        # Global Model Context Protocol tool servers
├── prompts/        # Global prompt templates
├── skills/         # Global skills (~/.agents/skills/<name>/SKILL.md)
└── workflows/      # Global workflows (see workflows.md)

<project>/.agents/
├── AGENTS.md       # Project instructions & engineering defaults
├── hooks/          # Project lifecycle hooks
├── mcp.json        # Project Model Context Protocol tool servers
├── prompts/        # Project prompt templates
├── skills/         # Project skills (.agents/skills/<name>/SKILL.md)
└── workflows/      # Project workflows (see workflows.md)

~/.config/rho/
├── auth.json       # Persisted credentials and OAuth tokens
├── config.toml     # Global application preferences and providers
└── permission.toml # Persisted global permission rules
```

Reload configuration at any time inside the REPL without dropping session
history:

```text
/reload
```

---

## Settings Reference

Every setting below can be set in `config.toml` (global or project), with `rho config <key> <value>`, or
through the environment variable shown. Numeric limits must be greater than zero; booleans accept
`true`/`false`, `yes`/`no`, or `1`/`0` in the environment and in `rho config` alike.

| Key | Environment variable | Default | Meaning |
| --- | --- | --- | --- |
| `max_output_tokens` | `AI_MAX_OUTPUT_TOKENS` | provider default | Cap on tokens per model reply |
| `max_turns` | `AI_MAX_TURNS` | 1000 | Model turns allowed in one run |
| `context_limit` | `AI_CONTEXT_LIMIT` | model's window | Override the context window size in tokens, for every model rho runs |
| `context_window_messages` | `AI_CONTEXT_WINDOW_MESSAGES` | 24 | Accepted and validated, but the engine does not currently read it |
| `compaction_max_bytes` | `AI_COMPACTION_MAX_BYTES` | 8192 | Size cap for a compaction summary |
| `reserve_tokens` | `AI_RESERVE_TOKENS` | 16384 | Compaction runs when the history passes the context limit minus this many tokens, before a turn or between its requests |
| `keep_recent_tokens` | `AI_KEEP_RECENT_TOKENS` | 20000 | Recent tokens kept verbatim by compaction |
| `context_injection_max_tokens` | `AI_CONTEXT_INJECTION_MAX_TOKENS` | 4000 | Accepted and validated, but the engine does not currently read it |
| `steering_mode` | `AI_STEERING_MODE` | `one-at-a-time` | Queued steering messages: `one-at-a-time` or `all` |
| `follow_up_mode` | `AI_FOLLOW_UP_MODE` | `one-at-a-time` | Queued follow-ups: `one-at-a-time` or `all` |
| `session_retention_days` | `AI_SESSION_RETENTION_DAYS` or `RHO_SESSION_RETENTION_DAYS` | 5 | Days to keep unused sessions; `off` or `0` in the environment disables pruning |
| `region` | `WEB_REGION` | `wt-wt` | Search region |
| `search_min_interval_ms` | n/a | 2000 | Minimum gap between web searches (0 means no throttling) |
| `search_timeout_sec` | n/a | 12 | Web search timeout |
| `fetch_timeout_sec` | n/a | 8 | Web fetch timeout |
| `fetch_limit` | n/a | 200 | Default number of lines `web_fetch` returns per page |
| `fetch_max_bytes` | n/a | 5000000 | Largest page body downloaded |
| `output_max_bytes` | n/a | 50000 | Largest MCP tool result returned inline; the rest is saved under `.rho/artifacts` |
| `allow_private_network` | `WEB_ALLOW_PRIVATE_NETWORK` | `false` | Let web tools reach private addresses (global file only) |

Terminal appearance can also be set with `RHO_CURSOR` (or `RHO_UI_CURSOR`) and `RHO_BLOCK_STYLE` (or
`RHO_UI_BLOCK_STYLE`). A zero or negative value for any limit above is rejected at startup with a message
naming the setting.

---

## Model & Guard Model Configuration

Configure model roles in `~/.config/rho/config.toml` or `.rho/config.toml` under the `[models]` table using the unified `<provider>/<model>` format:

```toml
# off, minimal, low, medium (default), high, xhigh, max, or auto; other values are rejected.
# "auto" classifies per-turn effort (low/medium/high) using models.judge (falling back to
# models.guard, or medium on timeout/unconfigured). Subagents use the same level (resolving
# auto to medium). Changing the level or model mid-session makes the next request rewrite
# the prompt cache; rho shows a notice when the cache is warm.
thinking_level = "high"

# Claude prompt-cache lifetime: auto (default: 1 hour on `claude` subscription
# logins, 5 minutes on `anthropic` API keys), short (5 minutes) or long (1 hour)
cache_retention = "auto"

[models]
# Primary assistant model (<provider>/<model>)
default = "anthropic/claude-3-7-sonnet"

# Dedicated local guard model for automated shell security classification
guard = "ollama/qwen2.5-coder:7b"

# Default model for subagents and workflow agents; also selected by `rho --smol`
smol = "ollama/qwen2.5-coder:7b"

# Deep-reasoning model selected by `rho --slow`
slow = "openai/o3-mini"

# Additional built-in roles (all optional)
judge = "anthropic/claude-3-7-sonnet"
widget = "ollama/qwen2.5-coder:7b"
review = "anthropic/claude-3-7-sonnet"
plan = "anthropic/claude-3-7-sonnet"
task = "ollama/qwen2.5-coder:7b"
vision = "anthropic/claude-3-7-sonnet"

# User-defined custom roles: any lowercase ASCII [a-z0-9_-]+ name except "default"
architect = "openai/o3-mini"
```

### Extensible `[models.<role>]` Roles & `@<role>` Selectors

`[models]` supports `default` (the primary assistant model), nine built-in roles (`smol`, `slow`, `guard`, `judge`, `widget`, `review`, `plan`, `task`, `vision`), and arbitrary user-defined custom roles (any lowercase ASCII `[a-z0-9_-]+` name other than `default`):

- **Configure any role** in `config.toml` under `[models.<role>]` or via `rho config models.<role> <provider>/<model>`. Set a role to `"none"` or an empty string to clear it.
- **`@<role>` selector syntax**: Pass `@<role>` wherever a model selector is accepted to resolve against `[models].<role>` (case-insensitive), or `@default` to resolve `[models].default`. Any selector not starting with `@` is used as a literal `<provider>/<model>` spec.
- **Fallback chains**: Subsystems that accept a role chain resolve the first configured role in order (for example, A2UI `decision_assist` resolves `models.widget` -> `models.judge` -> `models.guard`).

`cache_retention` sets how long Claude (`claude` and `anthropic` providers)
keeps each request's prompt cache. `auto` uses `long` for `claude`
subscription logins and `short` for `anthropic` API keys. Subagents always use
`short` under `auto`, since they finish within minutes. `long` writes cost
2x base input instead of 1.25x, but a prompt sent 5 to 60 minutes after the
last one reads the history from cache instead of writing it again. Override it
with `rho config cache_retention short` or `long`.

A spec without a `<provider>/` prefix uses the `--provider` flag when given,
then the provider implied by the model name (`claude-*` is `anthropic`,
`gpt-*` is `openai`, `gemini-*` is `gemini`, ...), then the current
provider (`local` at startup). During a session, a bare id the current
provider's live list carries stays on that provider instead, for `/model`,
`set_model`, and subagent model specs alike. Everything after the first `/` is the
provider's model id, so OpenRouter ids keep their vendor segment:
`openrouter/anthropic/claude-3.7-sonnet`. The prefix may name an account,
`claude:me@work.com/<model>` (see [Multiple Accounts](#multiple-accounts)).

### Dedicated Guard Model

The guard model evaluates shell commands before execution to determine whether they can run automatically or require interactive human authorization.

- **Non-Thinking & Deterministic**: The guard model strictly executes without thinking tokens (`thinking_level: off`) at `temperature: 0.0` to ensure fast, deterministic, sub-second security evaluation. Reasoning effort for the primary assistant remains governed by `thinking_level` (adjustable interactively via `/thinking` or `Shift+Tab`).
- **Recommended Local Model (`qwen2.5-coder:7b`)**: For fast, offline security checks without API costs or cloud latency, `ollama/qwen2.5-coder:7b` (or `local/qwen2.5-coder:7b`) is strongly recommended:
  - *Domain Comprehension*: Pretrained extensively on source code, shell scripts, CLI flags, and DevOps/SRE tooling (git, kubectl, terraform, docker, cloud CLIs, database clients), accurately discriminating benign developer commands from destructive mutations.
  - *Low Latency & Compact Footprint*: At ~4.7 GB quantized (Q4_K_M), it fits easily into consumer GPUs or Apple Silicon unified memory (M1-M4 with 8GB+ RAM), delivering sub-second evaluations without perceptible CLI lag.
  - *Deterministic Structured Output*: Reliably produces clean JSON adhering to `{"safe": boolean, "reason": "..."}` without conversational boilerplate or markdown fences.
  - *Air-Gapped Privacy*: Shell commands, repository paths, CLI arguments, and sensitive parameters remain strictly on-device.
  - *Offline Resilience*: Operates during network disruptions, in air-gapped corporate environments, or while traveling disconnected.

Configure via CLI:
```bash
# Pull model locally with Ollama
ollama pull qwen2.5-coder:7b

# Set guard model in rho
rho config models.guard ollama/qwen2.5-coder:7b

# Or disable guard model evaluation (reverts to baseline confirmation prompts)
rho config models.guard none
```

### Failed Requests

Each model request in a turn is retried on its own when it fails with a network error, a rate limit (HTTP 429), an overloaded or failing provider (HTTP 408, 500, 502, 503, 504, 529, or an `overloaded_error`/`api_error` stream event), a provider stream that goes idle for 300 seconds, or no response within 120 seconds: up to 3 attempts, announced as `[Model request failed (...); retrying in 2s (1/2)...]`. The wait honors the provider's `retry-after-ms` or `Retry-After` header (seconds or any HTTP date format), capped at 60 seconds, and otherwise backs off 2s then 4s. A 429 or 5xx that asks for a longer wait, or a hard quota (Antigravity's "Individual quota reached", OpenAI's `insufficient_quota`, ChatGPT's `usage_limit_reached`), fails immediately instead. Context-window overflows trigger compaction and are not retried; if the turn had already saved steps, its prompt and work are kept, so send a follow-up such as "continue" instead of resubmitting the prompt. Text streamed by the failed attempt may appear again. Each finished step of a turn (a model response and its tool results) is appended to the session file as soon as it completes, along with the tool audit events, so a failed turn, an Esc interrupt, a crash, or a killed terminal keeps that work as a checkpoint; the next prompt continues from it, and the work joins the conversation history when a turn completes or when the conversation is compacted (automatically, after a context overflow, or with `/compact`); a turn that stopped mid-tool gets a short note saying so. A step still waiting on its tools is dropped. Failed turns also record the error (`run_failed`) in the session log.

---

## Subagents & Custom Agents

The `subagent` tool hands a focused task to an isolated agent with its own
context. It returns the subagent's answer followed by evidence the parent can
check, and a footer with turn, tool-call, and token counts:

```text
Files changed: src/a.rs
Changed while its bash commands ran: src/fmt.rs (bash: cargo fmt), old.rs (deleted; bash: rm old.rs)
Also changed in the workspace during this run, not by its tools (editors, other agents, or processes): notes.md
Tool outcomes: 12 ok, 1 error (edit src/b.rs: oldText not found), 1 denied (bash rm -rf build)
```

`Files changed` comes from the subagent's successful `write` and `edit` calls,
with paths relative to the project. rho also snapshots the workspace before and
after each round and around each `bash` call, comparing file contents: changes
that appear while one of the subagent's `bash` commands runs are credited to
that command, and anything else that changed during the round is listed as
not made by its tools. In a git work tree the snapshot covers uncommitted files
(committing a file without changing it is not a change). Elsewhere rho walks
the project, respecting ignore files, for up to 150 ms and 20,000 files; when
that limit is hit the result says `Workspace check incomplete`. Snapshots cost
one `git status` per `bash` call. Inside a worktree (below) the subagent is
the only writer, so every change is its own; the "not by its tools" line only
appears outside git, where subagents run in the project directory. `Tool
outcomes` counts every call and
lists the ones that failed or were denied; the footer then reads
`1 failed tool call, ...`, a sign that the answer may describe work that did not
happen.

- **Worktrees**: in a git repository every subagent works in its own git
  worktree, checked out under your temp directory
  (`$TMPDIR/rho-worktrees-<uid>/<repository>/<id>`, private to you) on a
  branch `rho/wt/<id>`, and started from a snapshot of your working tree
  (uncommitted and untracked files included; your index and files are
  untouched). Its file
  tools, permission checks, and environment are rooted there, so its own files
  count as inside the workspace and a write into your working tree asks for
  permission. Top-level ignored entries (`target/`, `node_modules/`, `.env`)
  are symlinked in so builds stay warm. Nothing reaches your working tree until
  the main agent merges it: the result names the worktree and its changed
  files, and the `worktree` tool lists, diffs, merges (a three-way merge
  against your current files that leaves `<<<<<<<` conflict markers to fix),
  or discards it. `diff` opens with a
  static code review of the lines the worktree added and `merge` ends its result with one (see below). Review feedback with the task's `task_id` continues in the
  same worktree until it is merged or discarded; after that, a continued
  subagent gets a fresh worktree and is told so. A round that changes nothing
  leaves no worktree.
- **Worktree code review**: `worktree diff` starts with, and `worktree merge` ends with,
  `Review: N errors, M warnings in K files`, then `path:line [rule] message`
  findings (errors first, at most 30 shown) for the added lines only. Errors:
  git conflict markers, credential-looking strings, debug leftovers (`dbg!`,
  `debugger`, `breakpoint()`, `pdb.set_trace`), placeholders (`todo!()`,
  `unimplemented!()`, `NotImplementedError`). Warnings: lint suppressions
  (`#[allow]`, `eslint-disable`, `@ts-ignore`, `# noqa`, `//nolint`, ...),
  TODO/FIXME/XXX/HACK, `unwrap`/`expect` and `console.log` outside tests,
  commented-out code, lines over 140 characters, nesting deeper than 5 levels,
  functions adding over 60 lines or taking over 6 parameters or 2+ bool flags,
  new files over 400 lines, a string literal of 12+ characters repeated 3+
  times, and duplication (`path:START-END duplicates other:LINE (N lines)`:
  5+ significant added lines that also occur in another added file or in the
  checkout's other files, or a function whose body equals another's). Test
  code is exempt from the `unwrap`, `console.log`, placeholder, secret, length
  and nesting rules. The review is static: it does not run tests, linters, or
  cognitive-complexity checks, so run those. Findings never block a merge: send
  them to the subagent by continuing it, or fix them after merging.
- **Worktree cleanup**: the checkout is disposable; each round's work is
  committed to the worktree's branch, and its details are kept in
  `.git/rho/worktrees/<id>.json`, so a checkout removed by the OS's temp
  cleanup is checked out again when next needed. Cleanup happens at every
  level:
  - A round that ends normally commits its work and removes a worktree that
    changed nothing. A round that is aborted or crashes does the same in the
    background, and rho waits for those before exiting.
  - A worktree that fails partway through creation is removed at once.
  - The session that created a worktree owns it. When the main agent ends a
    turn while its session still owns unmerged worktrees, it is reminded once
    to merge or discard them or to tell you why one is kept.
  - The first time rho creates a worktree in a repository, it sweeps that
    repository: unmerged worktrees older than 24 hours are deleted (including
    any uncommitted edits in them), and
    branches, checkouts, and details files left by interrupted runs are
    removed (after a one-hour grace period, in case another rho process is
    still creating them). A worktree whose details file cannot be read is kept
    rather than removed.

- **Names**: pass `name` (camelCase, e.g. `performanceAudit`; other text such
  as `performance audit` is converted) to label the subagent's progress lines,
  its entry in the agent list, its task ids, and its result footer.
- **Conversations**: subagents started by the main agent talk to it using the
  [A2A](https://a2a-protocol.org/latest/specification/) task model (no server
  or network is involved). Each result ends with a task id and state, e.g.
  `[subagent performanceAudit · task performanceAudit-3f9a1c2e · completed: 12 turns, ...]`.
  The main agent reviews the work and can call `subagent` again with that
  `task_id` and feedback ("you missed X, fix it"); the subagent continues with
  everything it already read and did, in a new task of the same A2A context.
- **Inspecting**: `{"task_id": "...", "inspect": true}` shows a task's
  messages, final answer, every tool call with its outcome, the files it
  changed, and the files it has read, without running the subagent.
  `{"inspect": true}` lists the latest task of every saved subagent
  conversation, so task ids can be recovered after compaction.
- **Stale reads**: rho records a content fingerprint of every file a subagent
  reads or edits, including files named as arguments to its `bash` commands
  (`cat src/a.rs`). When the task is continued, any of those files that changed
  since are named at the top of the new message so the subagent re-reads them.
  In a git repository, rho also lists other files that differ from what the
  subagent last saw (up to 20), for example your edits after its worktree was
  merged, which covers what it saw through `rg` or other directory-wide
  commands. The result says what the subagent was told.
- **Finishing**: a subagent finishes by calling `complete_task` with its report,
  which becomes the task's result (an optional `data` object carries structured
  output as an A2A data part, saved as the task's `result-data` artifact); text it writes outside that call is not
  passed on. The call is refused while a message from the main agent is
  waiting or the subagent's own background work is still running, so the
  subagent reads or waits for it first. A subagent that ends with a plain
  reply instead has that reply used as its result.
- **Questions**: a subagent can call `ask_parent` when it cannot proceed
  without a decision. The question reaches the main agent as a message from
  the subagent, and the subagent waits with its work in progress. The main
  agent answers with the same `task_id`; the answer is the `ask_parent` call's
  result and the subagent continues in the same round. If the main agent waits
  or ends its turn instead of answering, the subagent pauses: its task becomes
  `input-required`, its result is `Question for you: ...`, and answering with
  the same `task_id` later resumes it with the reply as its next message. A
  message the user sends a waiting subagent from the agent view answers it the
  same way.
- **Same loop as the main agent**: a subagent runs on the main agent's turn
  loop with its own tools, instructions, and turn limit. It gets the same
  request retries, repeated-call guard, "Please continue" nudge after an empty
  reply, waiting for its own background `bash` jobs, message delivery, and
  context compaction: once its next request's prompt would pass its model's
  window minus `reserve_tokens` (or a request overflows the window), its older conversation
  is compacted into a summary (keeping `keep_recent_tokens` of recent
  conversation) and it continues with the turns it has left, up to three times
  per round.
- **Incomplete runs**: when a subagent hits its turn limit or ends without a
  final answer, it is asked once more (without tools) to summarize its work so
  far; the result starts with `Incomplete:` and the task is `input-required`,
  so replying `continue` with its `task_id` resumes it. If a model call fails,
  the task is `failed` but the findings gathered so far are still returned. The
  agent list shows waiting subagents as `needs input`.
- **Persistence**: each round is saved to the session file as a
  `subagent_task` record holding the A2A task and the subagent's new messages,
  so a task can be continued after `--resume`. Workflow agents are saved the
  same way (see [Workflows](workflows.md#hand-off)), but cannot call
  `ask_parent`: a workflow runs unattended.
- **Model**: without a `model` role, subagents use the role pinned for their
  persona, role, or agent in `[subagents.models]`, else `models.smol` (see
  below).

- **Built-in roles**: `scout` (read, fd, rg, lsp, web_fetch, web_search),
  `critic` and `planner` (read, fd, rg, lsp), and `general` (read, write, edit,
  undo, bash, fd, rg, lsp, web_fetch, web_search, todo, wait). Each subagent keeps its
  own `todo` list. An agent without `write` or `edit` gets `lsp` without the
  `rename` and `format` actions and without applying code actions, so read-only
  roles never change files through it.
- **Safety**: subagent tool calls go through the same permission rules and
  lifecycle hooks as the main agent, and `Esc` cancels them. (Agents inside a
  [workflow](workflows.md#trust) are trusted and skip both.)
- **Grounding**: every subagent's system prompt adds usage guidelines for its own
  tools, the `AGENTS.md` instructions, and the current directory, date,
  platform, and git status to the agent's own instructions.
- **Concurrency**: several subagent calls in one turn run in parallel. In a git
  repository that is every call, because each subagent has its own worktree;
  elsewhere only subagents whose tools are all read-only. Parallel subagents
  still share ignored build and dependency directories, ports, and the machine.
  Permission prompts still appear one at a time.
- **Background**: a subagent starts in the background by default. The call
  returns its task id at once (set `subagents.foreground_seconds` to a nonzero
  number to run it inline for that long first, and only then move it to the
  background); the main agent can keep working, call `wait` to block until a
  result arrives, or end its turn. The result arrives as a follow-up message
  when the subagent finishes, and the turn waits for it before ending. The tool
  card tells the user that it runs in the background and that `Left` on an empty
  prompt watches it (the model is not told this). Until
  then, inspecting that task says it is still running, and continuing it
  queues the prompt (with any `context_slice`) as a message the subagent reads
  before its next model call (also when it answers: it keeps working with the
  message), or, while it waits on an `ask_parent` question, answers that
  question. A `role`, `agent`, or `persona` that differs from the task's, or
  any `model` or `turns`, is refused instead of being ignored. A message is
  also refused, with the reason, when the subagent already has 32 unread
  messages, unless it answers the question the subagent is waiting on or is a
  system notice (such as a finished background command): those are always
  delivered. Messages it never read are listed with its
  result. A subagent
  running inline moves to the background as soon as it asks a question, so the
  main agent can answer. At most 8 subagents run at once;
  starting another is refused until one finishes. A result placed in a follow-up
  message is clipped at 20 KB (the session keeps the full record) and is marked
  as untrusted output, not instructions. Each result's footer names the model
  that ran. Its
  progress lines stop in the tool card, but `Left` still watches it live, and
  `Esc` cancels it with the turn. A background subagent belongs to the session
  that started it: only that session's turns wait for it and receive its
  result, and switching to another session (`/resume`, `/clear`) stops it.
- **Context**: `inherit_context: true` shares the recent text-only conversation
  with the subagent (and its model provider).
- **Watching**: press `Left` on an empty prompt to list subagents, and `Enter`
  to show one in the main terminal in place of the main agent, the way the main
  agent is shown (see [Workflows](workflows.md#watching-agents)). Text you type
  goes to it; `Left` on an empty prompt returns, and the main agent's output
  from meanwhile appears then.
- **Approvals**: when an agent you are not looking at needs a permission
  answer, the view switches to it (`approval_focus = "switch"`, the default).
  With `approval_focus = "indicate"`, the footer names the agent instead and
  its prompt opens when you show it.

### Custom agents

Define your own agents as markdown files. An agent file says what the agent
can do; its caller decides how a run goes (model and turn limit). The
frontmatter configures the agent, and the body is its system prompt:

```markdown
---
name: reviewer
description: Reviews diffs for correctness risks
tools: read, fd, rg
---
You are a strict reviewer. Report concrete bugs with file:line evidence.
```

| Field | Meaning |
|---|---|
| `name` | Name used to call the agent; defaults to the file name |
| `description` | Shown to the main agent when it picks a subagent |
| `tools` | Allowlist of tools; omit it to get the read-only set (`read`, `fd`, `rg`, `lsp` without `rename`, `format`, or code-action edits, `web_fetch`, `web_search`), so an agent that edits files or runs commands must list `write`, `edit`, or `bash` |
| `disallowed_tools` | Tools removed from the allowed set |

`model` and `max_turns` in agent files are ignored; pass them from the caller
instead.

Agents load from `~/.agents/agents/`, then `.claude/agents/` and
`.agents/agents/` in the project (subdirectories included). A later file with
the same `name` replaces an earlier one. Asking for an agent name or role that
doesn't exist is an error. Agents never get the `subagent` or `lsp` tool,
even when their `tools` list names them.

### Personas

A persona changes how a subagent behaves without changing what it can do. Its
body is added to the role's or agent's instructions under a `## Persona:
<name>` heading, and its tools still come from the role or agent. The main
agent picks one with the `subagent` tool's `persona` argument and sees each
persona's name and description in the tool description:

```markdown
---
name: skeptic
description: Assumes every claim is wrong until shown evidence
---
Treat every claim in the task as unverified. Ask for file:line evidence and
say plainly when something cannot be confirmed.
```

Personas load from `~/.agents/personas/`, then `.agents/personas/` in the
project (subdirectories included); a project persona replaces a user persona
with the same `name`. A persona works with any role or custom agent (for
example `critic` behavior with `general` tools), stays with the task when it is
continued, and asking for one that doesn't exist is an error. Workflow
`<agent>` blocks take one with `persona="..."` (see
[Workflows](workflows.md)).

### Which model runs

The caller picks a **model role** from settings, never a model name: the
`subagent` tool's `model` argument, or the `model` attribute of a workflow's
`<agent>` block.

| Role | Model |
|---|---|
| `default` | The main session's model |
| `smol` | `models.smol` |
| `slow` | `models.slow` |

Without a role, a subagent uses the role pinned for its persona, then for its
role or agent, in `[subagents.models]`, then `smol` if `models.smol` is set,
otherwise the main session's model. Asking
for `smol` or `slow` when it is not configured, or for any other name, is an
error. The turn limit is the caller's `turns` (default 100). A `model` or `turns`
passed when continuing a task is saved with it and used by later continuations.

Pin a model role per persona, built-in role, or custom agent name
(case-insensitive) in
`config.toml` or the project's `.rho/config.toml`; a project entry replaces the
user entry with the same name. Values must be `default`, `smol`, or `slow`, and
the pins also apply to workflow `<agent>` blocks without a `model`. A pin to a
role that is not configured fails with an error naming the pin, and keys that
match no role, agent, or persona are listed when the main agent inspects its
subagent tasks:

```toml
[subagents]
foreground_seconds = 0      # 0 (default): subagents start in the background
approval_focus = "switch"   # or "indicate"

[subagents.models]
scout = "smol"
critic = "slow"
reviewer = "default"
skeptic = "slow"
```

---

## UI & Block Framing

Configure block framing styles, visibility options, and border colors in
`~/.config/rho/config.toml`:

```toml
[ui]
# Block framing: "border" (outline, default), "solid" (fill), or "divider" (solid fill with top/bottom rules)
block_style = "border"

# Box assistant responses in bordered frames (default: false)
agent_block_output = false

# Hide thinking transcript blocks by default (default: false)
hide_thinking = false

# Expand tool output cards by default (default: false)
tools_expanded = false

# Cursor rendering: "hardware" (default, native terminal cursor) or "software" (reverse-video block)
cursor = "hardware"

# Syntax-highlight Guard/permission/question modals, bash/JSON tool headers, and tool outputs (default: false)
syntax_highlight_tools = false

# Display full bash commands in tool cards without 60-character truncation (default: false)
full_bash_command = false

# Render tool cards with a status + Title Case header, right-aligned duration, and explicit Input/Output sections (default: false)
structured_tool_cards = false

# Syntax-highlight edit diffs with delta-style red/green background tints and word-level emphasis (default: false)
delta_diffs = false

# Automatically summarize the session into a 3-6 word title using models.smol after turn 1 (default: false)
auto_title = false

# Publish the session title (or working directory name) to the terminal/tmux titlebar via OSC 0 (default: false)
terminal_title = false

# Prefix the terminal titlebar with live run state (idle/working/attention; default: false)
terminal_title_state = false

# Border colors (ANSI color names or "#rrggbb" hex; user defaults to "blue", others to "gray")
user_border = "blue"           # User prompt blocks
agent_border = "gray"          # Agent / sub-agent blocks
tool_border = "gray"           # General command / tool cards
bash_success_border = "gray"   # Successful bash commands
bash_error_border = "red"      # Failed bash commands

# Optional prompt chrome customization (default: 1-space indent, version shown, thinking-colored dividers)
[ui.prompt]
prefix = "❯ "                  # Custom prompt glyph (default: " ")
show_version = true            # Show version label in top divider (default: true)
divider_label = "ϱ"            # Optional custom top-divider label (overrides show_version)
divider_style = "thinking"     # "thinking" (default) | "dim" | "none"

# Optional segmented / powerline status bar (default: preset = "default" keeps the 2-line footer)
[ui.status_bar]
preset = "default"             # "default" | "minimal" | "compact" | "full" | "nerd" | "ascii" | "custom"
separator = "powerline-thin"   # "powerline" | "powerline-thin" | "dot" | "slash" | "pipe" | "block" | "none" | "ascii"
icons = "nerd"                 # "nerd" | "unicode" | "ascii"
transparent = false            # Drop background fill and powerline end caps
compact_thinking_level = true  # Show thinking effort as a single glyph next to the model
context_line = "off"           # Bottom divider gauge: "off" | "percentage" | "annotated" | "embedded"
left = ["model", "path", "git"]
right = ["token_total", "cost", "context_pct", "quota"]

# Per-segment options for [ui.status_bar]
[ui.status_bar.segment_options.path]
abbreviate = true
max_length = 40

[ui.status_bar.segment_options.git]
show_branch = true
show_staged = true
show_unstaged = true
show_untracked = true

[ui.status_bar.segment_options.model]
show_thinking_level = true
short_name = true
# Optional custom display aliases: [ui.status_bar.segment_options.model.names]
# gemini-4-neon = "Neo4"

[ui.status_bar.segment_options.time]
format = "24h"                 # "12h" | "24h"
show_seconds = false

# Optional per-turn telemetry rows below completed assistant responses (all default: false)
[ui.display]
show_token_usage = false       # Timestamp, tokens, TTFT, and right-aligned tok/s
show_turn_time = false         # Include prompt-to-yield Δ duration in the stats row
cache_miss_marker = false      # Show a divider when a warm prompt cache invalidates

# Opt-in A2UI v1.0 persistent TUI widgets (default: false)
a2ui_widgets = false

[ui.a2ui]
max_above_lines = 8            # Max lines rendered above the editor
max_below_lines = 4            # Max lines rendered below the editor
todo_widget = true             # Enable built-in `rho:todo` surface when a2ui_widgets = true
subagent_widget = true         # Enable built-in `rho:subagents` surface when a2ui_widgets = true
decision_assist = false        # Optional DecisionModel assist via models.widget -> models.judge -> models.guard
```

Top-level preferences:

- `allow_private_network = true`: Permit connections to loopback and private
  network addresses.
- `show_label = true`: Display the agent branding banner in the divider.

All adjustments made in the interactive `/settings` modal (Block Style, Box
Responses, Cursor Style, Model, Guard Model, Thinking Effort, Thinking Output,
Tool Output, and Version Banner) are automatically saved to `~/.config/rho/config.toml`.

Environment override: `RHO_BLOCK_STYLE=border` or `RHO_UI_BLOCK_STYLE=border`. Pass `--no-title` (or set `RHO_NO_TITLE=1`) to disable automatic session titling and terminal titlebar updates for a run.

---

## Web Tools, Search Providers & Permissions

Configure built-in web tools, search engines, MCP, and permission guards in `~/.config/rho/config.toml` or `.rho/config.toml` (or interactively via `/settings` under **Tools & Permissions**):

```toml
[tools.web.search]
# Enable or disable the built-in web_search tool (default: true)
enabled = true

# Primary search engine (default: "brave")
default = "brave"

# Ordered fallback engines to attempt if prior engines fail or return zero results
fallback = ["duckduckgo", "yahoo"]

[tools.web.fetch]
# Enable or disable the built-in web_fetch tool (default: true)
enabled = true

[tools.bash]
# Automatically load allowed `.envrc` files via `direnv export json` (default: false)
direnv = false
# Maximum time in ms to wait for `direnv export json` (default: 30000)
direnv_timeout_ms = 30000

[mcp]
# Enable or disable MCP subsystem (default: true)
enabled = true
# Longest wait for uncached MCP servers at startup, in ms; 0 waits for all (default: 3000)
startup_timeout_ms = 3000

[permission]
# Enable or disable runtime permission guardrails (default: true)
enabled = true
# Route all non-baseline Ask tool calls (MCP, out-of-workspace writes, etc.) through the guard model (default: false)
guard_all_tools = false
# Append JSONL guard decision audit records to ~/.config/rho/guard/decisions.jsonl (default: false)
guard_audit_log = false
```

Supported search engine identifiers:
- `brave`: Scrapes Brave search results (no API key required).
- `duckduckgo` (aliases: `ddg`, `ddg_lite`): Queries DuckDuckGo Lite (no API key required).
- `yahoo`: Scrapes Yahoo search results (no API key required).
- `exa`: Exa neural search. Uses the direct Exa Search API (`https://api.exa.ai/search`) when an API key is configured via `/login exa-search` (`auth.json`) or `EXA_API_KEY`, and automatically falls back to Exa's keyless public MCP endpoint (`https://mcp.exa.ai/mcp`) when no key is configured.
Searches evaluate engines sequentially in order. Once an engine returns results, search terminates immediately. Subsequent engines in the fallback list are only queried if the earlier engine errors or returns zero results.

To restrict searches to a single trusted engine and disable all fallbacks/scrapers:

```toml
[tools.web.search]
default = "brave"
fallback = []
```

When `enabled = false` for `web_search` or `web_fetch`, the tool is omitted from LLM dynamic registration and model tool catalogs. Tool toggles and default search engine selection can be changed interactively without leaving the session via `/settings` -> **Tools & Permissions**.

## Worktree Settings

Managed session worktrees (created by `/wt` and `worktree(action="create")`) are configured in the
`[worktree]` table of `~/.config/rho/config.toml`. Like `permission` and `providers`, this table is read
from the global file only.

```toml
[worktree]
# Directory under which managed session worktrees are created (default: ~/.config/rho/wt)
base = "/Users/me/worktrees"
# After `/wt` carries your changes into the new worktree, run `git reset --hard HEAD` and
# `git clean -fd` in the source checkout (default: false)
clean_source = false
```

`clean_source = true` discards uncommitted changes in the source checkout, so enable it only when you want
`/wt` to move work rather than copy it.

## Session Recap

Opt-in session recaps are configured under the `[recap]` table in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[recap]
# Generate a brief recap when returning to an idle session (default: false)
enabled = false
# Seconds of inactivity before an idle recap is triggered; clamped to 1..=3600 (default: 240)
idle_seconds = 240
# Generate a recap when resuming a previous session (default: false)
on_resume = false
```

You can also configure these keys with `rho config recap.enabled true`, `rho config recap.idle_seconds 240`, and `rho config recap.on_resume true`.

## Language Servers

The `lsp` tool, `read` with `symbol`, and the diagnostics at the end of `edit`
and `write` results use the language server for the file's extension. A server
runs only when its command is on `PATH` and a root marker is found: the
workspace root when it holds a marker (one server for the whole repository),
else the nearest ancestor of the file that does. In a trusted project
(`RHO_TRUST_PROJECT=1`), servers start on first use (reading a file starts its
server in the background, and `edit`/`write` start the servers they report
from). In a project that is not trusted, only an explicit `lsp` call or `read`
with `symbol` starts a server: reading or editing a file never does, because a
server analyzes the project, which for some languages runs its code. Servers
start without provider API keys, tokens, and secrets from rho's environment.
Servers stay up until unused for 10 minutes (`idleTimeoutMs` in `lsp.json`;
`0` never stops them), at most 8 run at once (the least recently used stops
first), and all are asked to shut down when rho exits. A server that fails to
start is retried after a minute; one that exits is restarted on next use, but
one that exits again within a minute of a restart waits out the minute. Each
server keeps at most 64 documents open. Edits to `lsp.json` apply within a few
seconds without restarting rho. Files outside
the workspace (such as a dependency a definition points into) are answered by
an already running server, never a new one. Subagents working in their own git
worktrees start their own servers for the worktree on first use (never
automatically in a project that is not trusted) and shut them down when the
subagent's round ends. A subagent without `write` or `edit` cannot `rename`,
`format`, or apply code actions. Like an editor, a language server
analyzes the project, which for some languages runs its build scripts. Set
`tools.lsp.enabled = false` (or `rho config tools.lsp.enabled false`) to remove
the `lsp` tool and run `read`, `edit`, and `write` without language servers.

Built in: rust-analyzer, gopls, typescript-language-server, pyright,
basedpyright, clangd, zls, lua-language-server, jdtls, kotlin-language-server,
ruby-lsp, sourcekit-lsp, haskell-language-server, ocamllsp, csharp-ls,
elixir-ls, intelephense, bash-language-server, terraform-ls, nil, and taplo
(the last three run in git repositories). When several servers handle an
extension, the first installed one answers; servers marked `linter` add their
diagnostics alongside it.

Add or change servers in `~/.agents/lsp.json`, `.lsp.json`, or
`.agents/lsp.json` (later files win, field by field per server name). The
project files choose which commands run, so they apply only when the project is
trusted (`RHO_TRUST_PROJECT=1`); otherwise `lsp status` says they were ignored.
A top-level `"enabled": false` turns every server off; a top-level
`"idleTimeoutMs"` sets how long an unused server stays up. The format matches
Claude Code's `.lsp.json`:

```json
{
  "rust-analyzer": {
    "settings": { "rust-analyzer": { "check": { "command": "clippy" } } }
  },
  "gopls": { "disabled": true },
  "ruff": {
    "command": "ruff",
    "args": ["server"],
    "extensions": ["py"],
    "rootMarkers": ["pyproject.toml"],
    "linter": true
  }
}
```

Fields: `command`, `args`, `extensions` (or `extensionToLanguage`, mapping
`.ext` to a language id), `rootMarkers` (file names, or `*.ext`),
`initializationOptions`, `settings` (answered to `workspace/configuration` and
sent with `didChangeConfiguration`), `env`, `disabled`, and `linter`.

### Adding a language server

To add a language rho does not know (Dart, for example):

1. Install the server so its command runs from your shell. Dart and Flutter
   ship it as `dart language-server`.
2. Add an entry to `~/.agents/lsp.json`, which applies to every project
   without needing trust:

   ```json
   {
     "dart": {
       "command": "dart",
       "args": ["language-server"],
       "extensions": ["dart"],
       "rootMarkers": ["pubspec.yaml"]
     }
   }
   ```

3. Run `lsp status` (or ask the agent to). `Installed:` lists the servers whose
   command resolves, with their extensions; a server that is not there shows
   under `Not on PATH:`. Then try `lsp symbols path=lib/main.dart`. If the file
   has no server, the error says whether the command is missing or no root
   marker was found above the file, and a server that fails to start shows the
   last lines it wrote to stderr (a `mise` or `asdf` shim with no version set
   shows up this way).

What each field does for a new server:

- `command` is required, and is looked up on `PATH` (or use an absolute path).
- `extensions` (with or without the dot) decides which files it handles. Without
  `extensions` or `extensionToLanguage` it never matches a file.
- `rootMarkers` pick the project root: the workspace root when it holds a
  marker, else the nearest ancestor of the file that does, so a monorepo whose
  root has no `pubspec.yaml` gets one server per package. With no markers the
  server runs at the workspace root.
- The language id sent to the server comes from `extensionToLanguage`
  (`{ ".dart": "dart" }`), then rho's built-in table, then the extension itself,
  which is already right for Dart. Set it only when the server expects another
  id.
- `settings` holds the server's own configuration, for example Dart's `dart.*`
  options from its documentation; rho answers the server's
  `workspace/configuration` requests from it.

Use `.agents/lsp.json` or `.lsp.json` in a project only for servers that should
run there for everyone; they apply only to trusted projects. To change a
built-in server, repeat just its name and the fields to change; `"disabled": true`
turns one off. A server whose rename needs file operations (create, rename,
delete) is refused for that action: rho applies text edits only.

After `edit` or `write`, rho sends the new content to every server for the
file and waits up to 3 seconds for its diagnostics. The result ends with the
file's errors and warnings (20 at most, errors first), a count of errors other
files report, or a note when a server has not answered yet. `lsp diagnostics`
waits up to 10 seconds. When the 3 seconds pass with the server still starting,
busy, or silent, rho keeps waiting in the background for up to 12 more seconds
and, if different diagnostics arrive and the file was not changed again,
delivers them to the model as a notice before its next step (framed as
untrusted tool output, like a background job's notice). Subagents without a
mailbox are told to run `lsp diagnostics` instead.

Servers also hear about files rho changes through
`workspace/didChangeWatchedFiles` (created, changed, or deleted): after `edit`,
`write`, `undo`, `lsp rename`, `lsp code_actions`, and `lsp format`, and when a
query finds an open document changed on disk.

`lsp code_actions path=... line=...` lists the server's quick fixes and
refactors for the diagnostics on that line (`symbol` narrows it to one
position). With `apply: true` and `query` (the listed number or part of the
title), the chosen action's edit is applied through the same safe path as
`rename`: every file must be inside the workspace and not a protected path,
all files are written or none, each is recorded for `undo`, and the servers are
told. The edit is fetched with `codeAction/resolve` when the list omitted it,
and actions that only run a server command are refused. `lsp format path=...`
asks the server to format the file with the `.editorconfig` indent (`tab`
style and size for `[*]`, `[*.ext]`, `[*.{a,b}]`, or the exact file name; else 4
spaces) and reports the changed regions. rho never formats on write.

A query that comes back empty within 30 seconds of a server starting, from a
server that never reported progress or status, ends with a note that the server
may still be indexing; retry in a few seconds.

## System Instructions (`AGENTS.md`)

`rho` discovers instructions hierarchically and prepends them into the agent
system context:

1. **Global User**: `~/.agents/AGENTS.md`
2. **Project Base**: `.agents/AGENTS.md`
3. **Workspace Context**: `./AGENTS.md`, `./CLAUDE.md`, or `./.cursorrules`

To disable context file discovery for a session, pass `--no-context-files` (or
`--nc`). This also skips loading memory notes.

## Memory Notes

The built-in `memory` tool lets the model keep short notes across sessions,
such as "fidelity tests are slow; run X first" or "file Y is high risk". Notes
are plain markdown files you can read and edit, one `- ` bullet per note (other
lines are ignored and dropped when the tool next saves the file):

- **Project**: `~/.agents/memory/<project key>.md`, where the key is the
  workspace's canonical path with every character other than letters, digits,
  `.` and `_` replaced by `-` (last 80 characters) plus a short hash of the
  full path, e.g. `Users-me-src-app-1a2b3c4d.md`.
- **Global**: `~/.agents/memory/global.md`, for every project.

The tool's actions are `view` (numbered notes per scope), `add` (`text`: one
line of at most 300 characters; duplicates are rejected) and `remove` (`index`
from `view`); `scope` is `project` (default) or `global`. Each scope holds at
most 50 notes or 8 KB; when it is full, the model must remove stale notes
first. Notes are never written into the repository, and memory calls need no
permission prompt (a `memory` rule in `permission.toml` still applies).
Subagents see the notes but do not get the tool.

Both files are read once when a session starts and added to the system prompt
in a `<memory>` block, so the prompt stays the same for the whole session (and
its prompt cache stays valid). Notes added or removed mid-session take effect
in the next session.

---

## Declarative Skills (`SKILL.md`)

Skills are reusable workflow templates that define instructions and parameter
hints.

### Discovery & Precedence

Skills are scanned from:

1. **Project Directory**: `.agents/skills/` or `./skills/`
   (highest precedence; overrides user skills)
2. **Global User Directory**: `~/.agents/skills/`

### Authoring Skills

A skill can be defined as either:

- A single markdown file: `skills/cleanup.md`
- A directory with a manifest: `skills/cleanup/SKILL.md` (can include adjacent
  helper scripts and docs)

Add YAML frontmatter to describe the skill and its arguments:

```markdown
---
name: plan
description: Explore the codebase and deliver an implementation plan.
argument-hint: "<task-description-or-spec>"
---

# Instructions for planning...
```

### Invoking Skills

Invoke skills in the interactive REPL:

```text
/skill:plan my-feature
```

Auto-completion presents matching skills as soon as you type `/skill `.

---

## Prompt Templates (`.agents/prompts/`)

Prompt templates are user-facing slash-command shortcuts and macros.

### Discovery & Precedence

Prompt templates are discovered across:

1. **Project Directory**: `.agents/prompts/` (overrides root `./prompts/` and user templates)
2. **Project Root**: `./prompts/`
3. **User Home**: `~/.agents/prompts/` (overrides `~/.config/rho/prompts/`)
4. **User Config**: `~/.config/rho/prompts/`

### Authoring Prompt Templates

Create a markdown file (e.g. `.agents/prompts/review.md`):

```markdown
---
description: "Review a file for safety and test coverage"
argument-hint: "<file-path>"
---
Please review ${1} for potential edge cases, security issues, and test coverage:
$ARGUMENTS
```

Supports variable expansion such as `$1`, `$2`, `$ARGUMENTS`, and `${1:-default}`.

### Invoking Templates

Templates are automatically registered as slash commands in the REPL:

```text
/review src/lib.rs
```
