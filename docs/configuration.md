# Configuration, Providers & Skills

This guide covers provider authentication, custom models, system instructions,
and declarative skills in `rho`.

---

## Providers & Authentication

`rho` supports 14+ AI providers via subscription OAuth, API keys, and local
inference:

| Provider       | Auth Type          | Configuration / Login                             |
| :------------- | :----------------- | :------------------------------------------------ |
| `chatgpt`      | Subscription OAuth | `rho login chatgpt` (OAuth PKCE)                  |
| `copilot`      | Subscription OAuth | `rho login copilot` (GitHub device login)         |
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
entry to `~/.config/rho/config.toml` or project `.rho/config.toml`:

```toml
[providers.custom_endpoint]
base_url = "https://api.example.com/v1"
key_env = "CUSTOM_API_KEY" # optional; falls back to `rho login custom_endpoint`
```

Select your custom provider using:

```sh
rho --provider custom_endpoint --model my-model-id
```

Or switch in the REPL with `/model custom_endpoint:my-model-id`.

> **Security Note**: `base_url` requires `http` or `https`. Requests to private
> or loopback IP ranges are rejected unless explicitly allowed with
> `allow_private_network = true` in `config.toml` (or
> `WEB_ALLOW_PRIVATE_NETWORK=true`).

---

## Configuration Hierarchy

`rho` loads configuration by merging settings in order of precedence (highest to
lowest):

1. **CLI Flags** (`--model`, `--provider`, `--thinking`, `--no-permission`,
   etc.)
2. **Environment Variables** (`AI_MODEL`, `AI_PROVIDER`, `AI_THINKING_LEVEL`,
   `AI_MAX_OUTPUT_TOKENS`, `AI_MAX_TURNS`, `AI_CONTEXT_WINDOW_MESSAGES`,
   `AI_COMPACTION_MAX_BYTES`, `RHO_HOME`)
3. **Project Configuration** (`.rho/config.toml`)
4. **Global Configuration** (`~/.config/rho/config.toml`)

```text
~/.agents/
├── AGENTS.md       # Global instructions & engineering defaults
├── hooks/          # Global lifecycle hooks (fallback)
├── mcp.json        # Global Model Context Protocol tool servers
├── prompts/        # Global prompt templates
└── skills/         # Global skills (~/.agents/skills/<name>/SKILL.md)

<project>/.agents/
├── AGENTS.md       # Project instructions & engineering defaults
├── hooks/          # Project lifecycle hooks
├── mcp.json        # Project Model Context Protocol tool servers
├── prompts/        # Project prompt templates
└── skills/         # Project skills (.agents/skills/<name>/SKILL.md)

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
- `semantic_search = true`: Enable local ONNX embeddings and passive RAG code retrieval across turns (default: `false`). Can also be set as `[features] semantic_search = true`.

All adjustments made in the interactive `/settings` modal (Block Style, Box
Responses, Cursor Style, Model, Semantic Search, Thinking Effort, Thinking Output,
Tool Output, and Version Banner) are automatically saved to `~/.config/rho/config.toml`.

Environment override: `RHO_BLOCK_STYLE=border` or `RHO_UI_BLOCK_STYLE=border`.

---

## Web Search Providers

Configure deterministic search engine priority and fallback for the built-in `web_search` tool in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[tools.web.search]
# Primary search engine (default: "brave")
default = "brave"

# Ordered fallback engines to attempt if prior engines fail or return zero results
fallback = ["duckduckgo", "yahoo"]
```

Supported engine identifiers:
- `brave`: Scrapes Brave search results.
- `duckduckgo` (aliases: `ddg`, `ddg_lite`): Queries DuckDuckGo Lite.
- `yahoo`: Scrapes Yahoo search results.
- `firecrawl`: Queries Firecrawl search API (requires `FIRECRAWL_API_KEY`).

Searches evaluate engines sequentially in order. Once an engine returns results, search terminates immediately. Subsequent engines in the fallback list are only queried if the earlier engine errors or returns zero results.

To restrict searches to a single trusted engine and disable all fallbacks/scrapers:

```toml
[tools.web.search]
default = "brave"
fallback = []
```

---

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

---

## Remote Daemon & P2P Access (`rho serve` and `/remote`)

`rho` can run headless as a persistent background daemon (e.g. via `systemd` or
`launchd`) using [Iroh](https://iroh.computer) peer-to-peer transport:

```bash
# Start background node for a repository
rho serve --workspace ~/src/backend-api

# Optional: specify custom port and friendly node name
rho serve --workspace ~/src/backend-api --port 50051 --name "work-laptop"
```

The node outputs a pairing URL and QR code. Open the URL in any browser to
access the **Fleet Hub** (`www/hub/`), or type `/remote` inside an active
terminal REPL session to pair on-demand. The Fleet Hub provides full parity with
the terminal interface, streaming thinking and tool executions, interactive
approvals, real-time token tracking, and live provider rate limit/quota status.
