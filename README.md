# rho

`rho` is a fast, clean, minimal coding-agent CLI built in Rust on Rig 0.42.

## Quick Start

```sh
# Start interactive REPL
rho

# Run one-shot prompt
rho -p "summarize this repository"

# Select model & provider
rho --provider anthropic --model claude-3-7-sonnet-20250219
rho --provider openai --model gpt-4o

# Resume existing session
rho --resume <SESSION_ID>
```

---

## Interactive Editor and Footer

When both stdin and stdout are terminals, `rho` runs an interactive editor with
a two-line status footer pinned to the bottom of the terminal screen:

```text
agent output remains above in normal scrollback
─────────────────────────────────────────────────────────
Write a message here; wrapped lines and explicit
newlines grow the editor upward.
─────────────────────────────────────────────────────────
~/src/github.com/casonadams/rho (main)
↑6.9k ↓514 5.4%/128k @14.3t/s      qwen3.8:27b-mlx • high
```

The **top line** shows the working directory, git branch, and session name. The
**stats line** shows tokens sent (`↑`) and received (`↓`), cache reads/writes
(`R`/`W`), spend (`$`), context usage (`%/window`), and generation speed
(`@t/s`) as they become available, with the active model and thinking level
right-aligned. The activity spinner animates in-place on the working line while
the model is thinking or executing tools, keeping the footer stable and
preventing terminal jitter.

| Control       | Behavior                                                                     |
| ------------- | ---------------------------------------------------------------------------- |
| `Enter`       | Submit prompt.                                                               |
| `Shift+Enter` | Insert a newline without submitting.                                         |
| `Ctrl+J`      | Insert a newline, including in terminals that encode it as a raw line feed.  |
| `Alt+Enter`   | Submit with follow-up queueing.                                              |
| `Escape`      | Clear an idle draft, or cancel active execution and restore queued messages. |

Messages submitted while the agent is running enter a FIFO queue and execute in
order once the active turn settles.

---

## Core Built-in Tools

`rho` includes 6 native, robust built-in tools:

- `read`: Read file contents with line numbering, offset, and limit safeguards.
- `write`: Create or overwrite files (automatically creates parent directories).
- `edit`: Make precise file edits with exact text replacement.
- `bash`: Execute shell commands in the current working directory.
- `search`: Search the web and return structured summaries and URLs.
- `fetch`: Fetch and extract clean readable text or markdown from URLs.

---

## Providers & Authentication

| Provider     | Auth Type     | Environment Variable / Login                     |
| ------------ | ------------- | ------------------------------------------------ |
| `anthropic`  | API Key       | `ANTHROPIC_API_KEY` or `rho login anthropic`     |
| `openai`     | API Key       | `OPENAI_API_KEY` or `rho login openai`           |
| `deepseek`   | API Key       | `DEEPSEEK_API_KEY` or `rho login deepseek`       |
| `gemini`     | API Key       | `GEMINI_API_KEY` or `rho login gemini`           |
| `groq`       | API Key       | `GROQ_API_KEY` or `rho login groq`               |
| `openrouter` | API Key       | `OPENROUTER_API_KEY` or `rho login openrouter`   |
| `xai`        | API Key       | `XAI_API_KEY` or `rho login xai`                 |
| `mistral`    | API Key       | `MISTRAL_API_KEY` or `rho login mistral`         |
| `cohere`     | API Key       | `COHERE_API_KEY` or `rho login cohere`           |
| `ollama`     | Local Service | `OLLAMA_HOST` (default `http://localhost:11434`) |

### Custom OpenAI-compatible providers

Any OpenAI-compatible endpoint can be added at runtime via config — no rebuild.
In `~/.config/rho/config.toml` (or project `.rho/config.toml`):

```toml
[providers.acme]
base_url = "https://api.acme.dev/v1"   # your keys are sent here
key_env = "ACME_API_KEY"               # optional; falls back to ACME_API_KEY env or `rho login acme`
```

Then select it with `/model acme:<model>`. Names of built-in providers are
reserved. For security, `base_url` must use `http`/`https` and private or
loopback addresses are rejected unless `allow_private_network = true` in
config.toml.

Slash commands `/reload` (re-read config, skills, and MCP tools without losing
session history) and `/export [html|md] [path]` (write the active branch as a
readable artifact) work in the interactive REPL.

---

## Configuration & Skills (`~/.config/rho/`)

Global settings, credentials, skills, and instructions live under
`~/.config/rho` (override via `RHO_HOME`):

```text
~/.config/rho/
├── auth.json              # Stored API keys
├── config.toml            # Application settings
├── AGENTS.md              # Global default agent rules & instructions
└── skills/                # Global skills directory (SKILL.md files)
```

- **Instructions**: Discovers global `~/.config/rho/AGENTS.md` and workspace
  `./AGENTS.md`, `./CLAUDE.md`, or `./.cursorrules`.
- **Skills**: Declarative `SKILL.md` workflows resolved from embedded built-ins,
  `~/.config/rho/skills/`, `.rho/skills/`, and `./skills/`.

---

## Model Context Protocol (MCP) Extensions

Extend `rho` with standard **MCP tool servers**:

```toml
# In ~/.config/rho/config.toml or .rho/config.toml
[mcp]
enabled = true

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/Users/username/Desktop"]
enabled = true

[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_..." }
enabled = true
```

Tools exposed by MCP servers are automatically namespaced
(`filesystem_read_file`, `github_create_issue`, etc.) and presented to the model
as standard tools.

---

## Plugins (Tool Hooks)

Plugins are small binaries that hook every tool call and can **allow**, **deny**
(with a reason sent to the model), or force an interactive approval prompt:

```toml
# In ~/.config/rho/config.toml or .rho/config.toml
[plugins.permission]
enabled = true
path = "/Users/username/.config/rho/plugins/rho-plugin-permission"
```

The first plugin is
**[rho-plugin-permission](https://github.com/casonadams/rho-plugin-permission)**:
allow/deny rules in `~/.config/rho/permission.toml` plus an interactive prompt
for everything else. Install it with:

```sh
rho plugin install rho-plugin-permission            # from crates.io
# or directly from a git repository:
rho plugin install https://github.com/casonadams/rho-plugin-permission
```

`rho plugin install` runs `cargo install` (from crates.io or a git URL), then
registers the plugin in `config.toml`; remove it again with `rho plugin remove
rho-plugin-permission`.

For the hook protocol, path resolution rules, and all options, see
**[docs/plugins.md](docs/plugins.md)**.

---

## Architecture

The workspace is structured into three clean, focused crates:

- **`rho-harness-core`**: Core domain logic, session DAG storage, configuration, token
  estimation, and presentation types.
- **`rho-engine`**: Native `rig.rs` agent runtime, provider factory, built-in
  tools (`read`, `write`, `edit`, `bash`, `search`, `fetch`), and standard MCP
  client.
- **`rho`**: Binary CLI entrypoint, interactive TUI, slash commands, and editor.
