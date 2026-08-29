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

## Interactive Editor and Footer

When both stdin and stdout are terminals, `rho` keeps a growing editor and a one-line footer at the bottom of the normal terminal screen:

```text
agent output remains above in normal scrollback
────────────────────────────────────────────────
Write a message here; wrapped lines and explicit
newlines grow the editor upward.
────────────────────────────────────────────────
⠋ thinking | gpt-5.6-luna | 2.9% (376k) | 74% (5d9h) | 2 queued
```

The footer reports current activity, model, context capacity, subscription quota/cooldown when available, and the queued-message count. Its activity indicator animates while the model is thinking or a tool is working.

| Control | Behavior |
| --- | --- |
| `Enter` | Submit with steering intent. |
| `Shift+Enter` | Insert a newline without submitting. |
| `Ctrl+J` | Insert a newline, including in terminals that encode it as a raw line feed. |
| `Alt+Enter` | Submit with follow-up intent. |
| `Escape` | Clear an idle draft, cancel active work and restore queued messages to the editor, or cancel the current modal interaction. |

Messages submitted while the agent is active enter one FIFO queue and run in submission order after the active turn finishes. Steering and follow-up labels preserve the intended delivery mode, but currently have the same timing because Rig 0.42 does not expose a safe mid-run steering boundary. If a terminal does not distinguish modified Enter keys, that input falls back to its reported behavior, typically plain `Enter` submission.

The live editor uses the normal screen rather than an alternate screen. Rho-owned streamed output is written above it and remains available in terminal-native scrollback. Output written directly to stdout by a plugin or subprocess bypasses Rho's terminal controller and may temporarily disrupt the live editor; arbitrary child-process output cannot be intercepted reliably.

If either stdin or stdout is not a TTY, `rho` falls back to the legacy line editor without the bottom live region or active-turn queueing. One-shot print mode is unchanged.

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
- **Skills**: Declarative `SKILL.md` workflows resolved from embedded built-ins, `~/.config/rho/skills/`, `.rho/skills/`, `./prompts/skills/`, and `./skills/`. Project and user overrides replace same-name built-ins (`/skill` lists each skill's origin); skill content is displayed or loaded as data and is never executed.

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
