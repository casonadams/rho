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
> `allow_private_network = true` in `config.toml` (or `WEB_ALLOW_PRIVATE_NETWORK=true`).

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
├── mcp.json        # Global Model Context Protocol tool servers
└── skills/         # Global skills (~/.agents/skills/<name>/SKILL.md)

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

Configure block framing styles, visibility options, and border colors in `~/.config/rho/config.toml`:

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

# Border colors (ANSI color names or "#rrggbb" hex; user defaults to "blue", others to "gray")
user_border = "blue"           # User prompt blocks
agent_border = "gray"          # Agent / sub-agent blocks
tool_border = "gray"           # General command / tool cards
bash_success_border = "gray"   # Successful bash commands
bash_error_border = "red"      # Failed bash commands
```

Top-level preferences:
- `allow_private_network = true`: Permit connections to loopback and private network addresses.
- `show_label = true`: Display the agent branding banner in the divider.

All adjustments made in the interactive `/settings` modal (Block Style, Box Responses, Model, Thinking Effort, Thinking Output, Tool Output, and Version Banner) are automatically saved to `~/.config/rho/config.toml`.

Environment override: `RHO_BLOCK_STYLE=border` or `RHO_UI_BLOCK_STYLE=border`.

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

1. **Project Directory**: `.agents/skills/`, `.rho/skills/`, or `./skills/`
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
