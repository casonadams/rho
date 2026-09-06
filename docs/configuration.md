# Configuration, Providers & Skills

This guide covers provider authentication, custom models, system instructions, and declarative skills in `rho`.

---

## Providers & Authentication

`rho` supports 14+ AI providers via subscription OAuth, API keys, and local inference:

| Provider | Auth Type | Configuration / Login |
| :--- | :--- | :--- |
| `chatgpt` | Subscription OAuth | `rho login chatgpt` (OAuth PKCE) |
| `copilot` | Subscription OAuth | `rho login copilot` (GitHub device login) |
| `antigravity` | Google OAuth | `rho login antigravity` (OAuth PKCE) |
| `claude` | Subscription OAuth | `rho login claude` (OAuth PKCE) |
| `openrouter` | OAuth or API Key | `OPENROUTER_API_KEY` or `rho login openrouter` |
| `anthropic` | API Key | `ANTHROPIC_API_KEY` or `rho login anthropic` |
| `openai` | API Key | `OPENAI_API_KEY` or `rho login openai` |
| `deepseek` | API Key | `DEEPSEEK_API_KEY` or `rho login deepseek` |
| `gemini` | API Key | `GEMINI_API_KEY` or `rho login gemini` |
| `groq` | API Key | `GROQ_API_KEY` or `rho login groq` |
| `xai` | API Key | `XAI_API_KEY` or `rho login xai` |
| `mistral` | API Key | `MISTRAL_API_KEY` or `rho login mistral` |
| `cohere` | API Key | `COHERE_API_KEY` or `rho login cohere` |
| `ollama-cloud` | API Key | `OLLAMA_API_KEY` or `rho login ollama-cloud` |
| `local` | Local Daemon | `OLLAMA_HOST` (default: `http://localhost:11434`) |

Credentials and tokens are stored securely in `~/.config/rho/auth.json` (or `$RHO_HOME/auth.json`).

### Custom OpenAI-Compatible Endpoints

Connect any OpenAI-compatible provider at runtime without rebuilding. Add an entry to `~/.config/rho/config.toml` or project `.rho/config.toml`:

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

> **Security Note**: `base_url` requires `http` or `https`. Requests to private or loopback IP ranges are rejected unless explicitly allowed with `allow_private_network = true` under the provider config.

---

## Configuration Hierarchy

`rho` loads configuration by merging settings in order of precedence (highest to lowest):

1. **CLI Flags** (`--model`, `--provider`, `--thinking`, `--no-permission`, etc.)
2. **Environment Variables** (`AI_MODEL`, `AI_PROVIDER`, `AI_THINKING_LEVEL`, `RHO_HOME`)
3. **Project Configuration** (`.rho/config.toml`)
4. **Global Configuration** (`~/.config/rho/config.toml`)

```text
~/.config/rho/
├── auth.json       # Persisted credentials and OAuth tokens
├── config.toml     # Global application preferences and providers
├── permission.toml # Persisted global permission rules
└── themes/         # Custom color themes (*.toml)
```

Reload configuration at any time inside the REPL without dropping session history:

```text
/reload
```

---

## System Instructions (`AGENTS.md`)

`rho` discovers instructions hierarchically and prepends them into the agent system context:

1. **Global User**: `~/.agents/AGENTS.md`
2. **Project Base**: `.agents/AGENTS.md`
3. **Workspace Context**: `./AGENTS.md`, `./CLAUDE.md`, or `./.cursorrules`

To disable context file discovery for a session, pass `--no-context-files` (or `--nc`).

---

## Declarative Skills (`SKILL.md`)

Skills are reusable workflow templates that define instructions and parameter hints.

### Discovery & Precedence

Skills are scanned from:

1. **Project Directory**: `.agents/skills/`, `.rho/skills/`, or `./skills/` (highest precedence; overrides user skills)
2. **Global User Directory**: `~/.agents/skills/`

### Authoring Skills

A skill can be defined as either:
- A single markdown file: `skills/cleanup.md`
- A directory with a manifest: `skills/cleanup/SKILL.md` (can include adjacent helper scripts and docs)

Add YAML frontmatter to describe the skill and its arguments:

```markdown
---
name: create-plugin
description: Scaffold and package an MCP tool server or rho lifecycle plugin.
arguments:
  - name: plugin_name
    description: Target plugin identifier
    required: true
---

# Instructions for scaffolding plugin...
```

### Invoking Skills

Invoke skills in the interactive REPL:

```text
/skill:create-plugin my-plugin
```

Auto-completion presents matching skills as soon as you type `/skill `.
