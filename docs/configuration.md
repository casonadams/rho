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
context. It returns a summary with turn, tool-call, and token counts, plus the
files it changed with `write` or `edit` (`Files changed: ...`).

- **Names**: pass `name` (camelCase, e.g. `performanceAudit`; other text such
  as `performance audit` is converted) to label the subagent's progress lines,
  its entry in the agent list, and its result footer
  (`[subagent performanceAudit: 12 turns, ...]`).
- **Incomplete runs**: when a subagent hits its turn limit, ends without a final
  answer, or loses its model mid-run, it is asked once more (without tools) to
  summarize its work so far. The result starts with `Incomplete:` and its
  footer says `incomplete`; if that summary also fails, the findings gathered
  so far are returned instead. The agent list shows these runs as
  `incomplete`.

- **Built-in roles**: `scout` (read, fd, rg, web_fetch, web_search), `critic`
  and `planner` (read, fd, rg), and `general` (read, write, edit, bash, fd, rg,
  web_fetch, web_search).
- **Safety**: subagent tool calls go through the same permission rules and
  lifecycle hooks as the main agent, and `Esc` cancels them. (Agents inside a
  [workflow](workflows.md#trust) are trusted and skip both.)
- **Grounding**: every subagent's system prompt adds usage guidelines for its own
  tools, the `AGENTS.md` instructions, and the current directory, date,
  platform, and git status to the agent's own instructions.
- **Concurrency**: several subagent calls in one turn run in parallel when every
  tool they can use is read-only. Permission prompts still appear one at a time.
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

### Which model runs

The caller picks a **model role** from settings, never a model name: the
`subagent` tool's `model` argument, or the `model` attribute of a workflow's
`<agent>` block.

| Role | Model |
|---|---|
| `default` | The main session's model |
| `smol` | `models.smol` |
| `slow` | `models.slow` |

Without a role, a subagent uses `smol` if `models.smol` is set, otherwise the
main session's model. Asking for `smol` or `slow` when it is not configured, or
for any other name, is an error. The turn limit is the caller's `turns`
(default 100).

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
