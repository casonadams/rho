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
| `local`        | Local Daemon       | `OLLAMA_HOST` (default: `http://localhost:11434`) |

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
3. **Project Configuration** (`.rho/config.toml`): model and UI preferences only. `permission`, `providers`,
   `allow_private_network`, and MCP servers are read from the global file alone, so a cloned repository cannot change them.
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

## Model & Guard Model Configuration

Configure model roles in `~/.config/rho/config.toml` or `.rho/config.toml` under the `[models]` table using the unified `<provider>/<model>` format:

```toml
thinking_level = "high"

[models]
# Primary assistant model (<provider>/<model>)
default = "anthropic/claude-3-7-sonnet"

# Dedicated local guard model for automated shell security classification
guard = "ollama/qwen2.5-coder:7b"

# Default model for subagents and workflow agents; also selected by `rho --smol`
smol = "ollama/qwen2.5-coder:7b"

# Deep-reasoning model selected by `rho --slow`
slow = "openai/o3-mini"
```

### Dedicated Guard Model

The guard model evaluates shell commands before execution to determine whether they can run automatically or require interactive human authorization.

- **Non-Thinking & Deterministic**: The guard model strictly executes without thinking tokens (`thinking_level: None`) at `temperature: 0.0` to ensure fast, deterministic, sub-second security evaluation. Reasoning effort for the primary assistant remains governed by `thinking_level` (adjustable interactively via `/thinking` or `Shift+Tab`).
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
  or discards it. Review feedback with the task's `task_id` continues in the
  same worktree until it is merged or discarded; after that, a continued
  subagent gets a fresh worktree and is told so. A round that changes nothing
  leaves no worktree.
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
    repository: unmerged worktrees older than 24 hours are deleted, and
    branches, checkouts, and details files left by interrupted runs are
    removed (after a one-hour grace period, in case another rho process is
    still creating them).

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
- **Questions**: a subagent can call `ask_parent` when it cannot proceed
  without a decision. Its task becomes `input-required` and its result is
  `Question for you: ...`; the main agent answers with the same `task_id`, and
  the answer is delivered as the `ask_parent` result.
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

- **Built-in roles**: `scout` (read, fd, rg, web_fetch, web_search), `critic`
  and `planner` (read, fd, rg), and `general` (read, write, edit, bash, fd, rg,
  web_fetch, web_search).
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
- **Background**: a subagent still running after `subagents.foreground_seconds`
  (default 60) moves to the background. The call returns its task id at once so
  the main agent can keep working; the result arrives as a follow-up message
  when the subagent finishes, and the turn waits for it before ending. Until
  then, inspecting that task says it is still running, and continuing it
  queues the prompt (with any `context_slice`) as a message the subagent reads
  before its next model call (also when it answers: it keeps working with the
  message). A `role`, `agent`, or `persona` that differs from the task's, or
  any `model` or `turns`, is refused instead of being ignored. Messages it
  never read are listed with its result. At most 8 subagents run at once;
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
- **Watching**: press `Left` on an empty prompt to list subagents and watch one
  live (see [Workflows](workflows.md#watching-agents)).

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
| `tools` | Allowlist of tools; omit it to allow every tool |
| `disallowed_tools` | Tools removed from the allowed set |

`model` and `max_turns` in agent files are ignored; pass them from the caller
instead.

Agents load from `~/.agents/agents/`, then `.claude/agents/` and
`.agents/agents/` in the project (subdirectories included). A later file with
the same `name` replaces an earlier one. Asking for an agent name or role that
doesn't exist is an error. Agents never get the `subagent` tool themselves,
even without a `tools` list.

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
error. The turn limit is the caller's `turns` (default 100).

Pin a model role per persona, built-in role, or custom agent name
(case-insensitive) in
`config.toml` or the project's `.rho/config.toml`; a project entry replaces the
user entry with the same name. Values must be `default`, `smol`, or `slow`, and
the pins also apply to workflow `<agent>` blocks without a `model`:

```toml
[subagents]
foreground_seconds = 60

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
# Block framing: "border" (outline, default) or "solid" (fill)
block_style = "border"

# Box assistant responses in bordered frames (default: false)
agent_block_output = false

# Hide thinking transcript blocks by default (default: false)
hide_thinking = false

# Expand tool output cards by default (default: false)
tools_expanded = false

# Cursor rendering: "hardware" (default, native terminal cursor) or "software" (reverse-video block)
cursor = "hardware"

# Border colors (ANSI color names or "#rrggbb" hex; user defaults to "blue", others to "gray")
user_border = "blue"           # User prompt blocks
agent_border = "gray"          # Agent / sub-agent blocks
tool_border = "gray"           # General command / tool cards
bash_success_border = "gray"   # Successful bash commands
bash_error_border = "red"      # Failed bash commands
```

Top-level preferences:

- `allow_private_network = true`: Permit connections to loopback and private
  network addresses.
- `show_label = true`: Display the agent branding banner in the divider.

All adjustments made in the interactive `/settings` modal (Block Style, Box
Responses, Cursor Style, Model, Guard Model, Thinking Effort, Thinking Output,
Tool Output, and Version Banner) are automatically saved to `~/.config/rho/config.toml`.

Environment override: `RHO_BLOCK_STYLE=border` or `RHO_UI_BLOCK_STYLE=border`.

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
[mcp]
# Enable or disable MCP subsystem (default: true)
enabled = true
# Longest wait for uncached MCP servers at startup, in ms; 0 waits for all (default: 3000)
startup_timeout_ms = 3000

[permission]
# Enable or disable runtime permission guardrails (default: true)
enabled = true
```

Supported search engine identifiers:
- `brave`: Scrapes Brave search results (no API key required).
- `duckduckgo` (aliases: `ddg`, `ddg_lite`): Queries DuckDuckGo Lite (no API key required).
- `yahoo`: Scrapes Yahoo search results (no API key required).
Searches evaluate engines sequentially in order. Once an engine returns results, search terminates immediately. Subsequent engines in the fallback list are only queried if the earlier engine errors or returns zero results.

To restrict searches to a single trusted engine and disable all fallbacks/scrapers:

```toml
[tools.web.search]
default = "brave"
fallback = []
```

When `enabled = false` for `web_search` or `web_fetch`, the tool is omitted from LLM dynamic registration and model tool catalogs. Tool toggles and default search engine selection can be changed interactively without leaving the session via `/settings` -> **Tools & Permissions**.

## System Instructions (`AGENTS.md`)

`rho` discovers instructions hierarchically and prepends them into the agent
system context:

1. **Global User**: `~/.agents/AGENTS.md`
2. **Project Base**: `.agents/AGENTS.md`
3. **Workspace Context**: `./AGENTS.md`, `./CLAUDE.md`, or `./.cursorrules`

To disable context file discovery for a session, pass `--no-context-files` (or
`--nc`).

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
