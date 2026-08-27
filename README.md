# rho

`rho` is a fast, clean, and extensible local coding-agent CLI built on Rust and Rig 0.42.

## Quick Start

```sh
# Start interactive REPL
rho

# Run one-shot prompt
rho -p "summarize this repository"

# Select model & provider
rho --provider anthropic --model claude-sonnet-4-6
rho --provider chatgpt --model gpt-5.6-luna

# Resume existing session
rho --resume <SESSION_ID>
```

---

## Interactive Status Line

`rho` displays a lean, label-free status line tracking active context capacity and live subscription quotas:

```text
gpt-5.6-luna | 2.9% (376k) | 74% (5d9h)
claude-sonnet-4-6 | 27.4% (1M) | 93% (3h22m)
```

- **Format**: `<model> | <context % of ceiling> | <subscription usage & cooldown>`
- **Context Ceilings**: GPT-5 / Luna / Codex (376k), Claude (1M), Gemini (1M), DeepSeek (128k).
- **Subscription Quotas**: Queries rolling 5-hour and 7-day windows and cooldown timers directly from ChatGPT & Anthropic OAuth endpoints.

---

## Providers & Authentication

| Provider | Auth Type | Environment Variable / Login |
| --- | --- | --- |
| `anthropic` | API Key | `ANTHROPIC_API_KEY` or `rho login anthropic` |
| `openai` | API Key | `OPENAI_API_KEY` or `rho login openai` |
| `deepseek` | API Key | `DEEPSEEK_API_KEY` or `rho login deepseek` |
| `gemini` | API Key | `GEMINI_API_KEY` or `rho login gemini` |
| `groq` | API Key | `GROQ_API_KEY` or `rho login groq` |
| `openrouter` | API Key | `OPENROUTER_API_KEY` or `rho login openrouter` |
| `chatgpt` | Subscription OAuth | `rho login chatgpt` (OAuth device flow) |
| `copilot` | Subscription OAuth | `rho login copilot` (OAuth device flow) |
| `ollama` | Local Service | `OLLAMA_HOST` (default `http://localhost:11434`) |

---

## Configuration & Skills (`~/.config/rho/`)

Global settings, credentials, skills, and instructions live under `~/.config/rho` (override via `RHO_HOME`):

```text
~/.config/rho/
├── auth.json              # Stored API keys
├── config.toml            # Application settings
├── AGENTS.md              # Global default agent rules & instructions
├── skills/                # Global skills directory (SKILL.md files)
├── plugins/               # Global plugins and manifests
└── tokens/                # Provider OAuth tokens (ChatGPT, Copilot)
```

- **Instructions**: Discovers global `~/.config/rho/AGENTS.md` and workspace `./AGENTS.md`, `./CLAUDE.md`, or `./.cursorrules`.
- **Skills**: Discovers `~/.config/rho/skills/`, `.rho/skills/`, and `./skills/` for `SKILL.md` workflows.

---

## Plugins & Extensions

Install plugins published to crates.io with Cargo or manage them via `rho`:

```sh
# Install from crates.io
cargo install rho-plugin-review
# or:
rho plugin install rho-plugin-review

# List discovered plugins
rho plugin list
```

`rho` automatically discovers plugin binaries in `~/.cargo/bin/`, `$PATH`, `~/.config/rho/plugins/`, and `.rho/plugins/` matching `rho-plugin-<name>` or `rho-<name>`.

For full plugin development, lifecycle hooks (`Extension` trait), and manifest options, see **[docs/plugins.md](docs/plugins.md)**.

---

## Architecture

The runtime is organized around explicit boundaries:

- `AgentEngineBuilder` constructs provider models, sessions, tools, and extensions.
- `AgentEngine` coordinates turns and exposes session-facing state.
- `ToolRegistry` describes the typed tools and their capabilities; `ToolExecutionPolicy` remains the approval authority.
- Usage, quota, and context state are tracked independently from turn execution.
- Runner errors preserve distinct authentication, network, provider, session, policy, tool, cancellation, and budget categories.

These boundaries are also used by offline contract tests, so provider construction and tool policy can be tested without credentials or network access.

---


- **[Plugins & Extensions](docs/plugins.md)**: Plugin architecture, lifecycle hooks, and crates.io publishing guide.
- **[Release Readiness](docs/release-readiness.md)**: Smoke testing, verification invariants, and audit policies.
