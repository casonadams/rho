# Capability Inventory

Base inventory for the Core Plugin Host refactor (`.specs/core-plugin-host/`,
plan Slice 0). Every item below is verified against working-tree source at the
time of writing; per-slice deletions in the refactor must check capability loss
against this document (spec REQ-002). Source-of-truth files are cited per
section so the inventory can be re-verified mechanically as slices land.

Per REQUIREMENT: this inventory records capability *coverage*. Interfaces are
free to be redesigned during the refactor; silently dropping an item is a
defect. Deliberate redesigns are recorded in the implementing slice.

## 1. Provider capabilities -> `rho-plugin-providers`

Twelve providers (`src/engine/provider/id.rs` `ProviderId::ALL`, verified in
`src/engine/provider/{factory.rs,catalog.rs}`):

| Provider | id string | Credential strategy | Model catalog | Notes |
| --- | --- | --- | --- | --- |
| Anthropic | `anthropic` | API key (`ANTHROPIC_API_KEY`) | live listing; curated fallback | |
| OpenAI | `openai` | API key (`OPENAI_API_KEY`) | live listing; curated fallback | |
| ChatGPT | `chatgpt` | Subscription OAuth (`rho login chatgpt`, device flow) | curated | tokens under `<config>/tokens/` |
| GitHub Copilot | `copilot` | Subscription OAuth (`rho login copilot`, device flow) | live listing via copilot client | |
| DeepSeek | `deepseek` | API key (`DEEPSEEK_API_KEY`) | live listing; curated fallback | |
| Gemini | `gemini` (alias `google`) | API key (`GEMINI_API_KEY`) | live listing; curated fallback | |
| Groq | `groq` | API key (`GROQ_API_KEY`) | live listing; curated fallback | |
| Ollama | `ollama` | Local service (`OLLAMA_HOST`, default `http://localhost:11434`) | live listing | no credentials |
| OpenRouter | `openrouter` | API key (`OPENROUTER_API_KEY`) | live listing; curated fallback | |
| xAI | `xai` | API key (`XAI_API_KEY`) | curated | |
| Mistral | `mistral` | API key (`MISTRAL_API_KEY`) | live listing; curated fallback | |
| Cohere | `cohere` | API key (`COHERE_API_KEY`) | curated | |

Per-model context ceilings come from `src/plugin/provider.rs::context_limit`
(covered by the provider parity matrix test there). Quota windows (rolling
5-hour/7-day + cooldowns for subscription OAuth) are tracked host-side in the
engine; where the quota *source* code moves with the provider plugin, the
window state stays host-side.

## 2. Tool capabilities -> `rho-plugin-builtin`

Ten declarations / eight logical tools, from `DECLARATIONS` in
`src/plugin/builtin_tools.rs` (kinds, effects, and execution modes verified
there; policy classes enforced host-side by `ToolExecutionPolicy` +
`tools/approval/`):

| Tool (declared names) | Kind | Declared effects / policy class | Execution mode |
| --- | --- | --- | --- |
| `read` | Read-only | `ReadPath` | Parallel |
| `write` | Workspace mutation | `WritePath` | Sequential |
| `edit` | Workspace mutation | `WritePath` | Sequential |
| `bash` | Shell | `ExecuteProcess`; risk tier from `bash_ast` classification | Sequential |
| `websearch` + `web_search` (aliases) | Network | `Network` | Parallel |
| `webfetch` + `web_fetch` (aliases) | Network | `Network` | Parallel |
| `ask_user` + `ask_user_question` (aliases) | Interactive | `UserInteraction` | Sequential |

Implementation modules behind them (move with the plugin): `src/tools/{read,
write,edit,bash,ask_user}.rs`, `src/tools/web/{search,fetch,http.rs,
rate_limiter.rs}` (HTML/JSON/Markdown/RSS/Atom/CSV/PDF extraction), prompts from
`prompts/tools/*.md` (build-embedded model-facing guidance). Bash
command-safety classification (`bash_ast`: `RiskTier`/`SafetyAnalysis`/
`analyze_command_safety`) stays **host-side policy input** (host floor denies;
tool cannot widen it).

There is no `web_parse` tool; URL parsing is part of `webfetch`/`web_fetch`.

## 3. Command capabilities -> `rho-plugin-builtin`

From `SLASH_COMMANDS` + dispatch arms in `src/repl/commands.rs`:

| Command | Aliases | Behavior |
| --- | --- | --- |
| `/help` | | print help |
| `/clear` | `/reset` | reset conversation context |
| `/model` | | show or switch provider/model (also lists) |
| `/skill` | `/skills` | list resolved skills / print one |
| `/plugin` | `/plugins` | plugin management entry points |
| `/login` | | provider login (OAuth device flow or key entry) |
| `/logout` | | provider logout / token removal |
| `/exit` | `/quit` | leave the REPL |

`/echo` is not a built-in; it appears only in a test proving unknown commands
route to the legacy extension surface (`src/repl/commands.rs` echo-extension
test). The legacy surface is deleted in Slice 6 (spec REQ-013).

## 4. Lifecycle capabilities (host event surface)

From `src/plugin/contract.rs` and `src/plugin/hook.rs`:

- `LifecycleEvent` variants: `HostStarted`, `SessionStarted`, `BeforeTurn`,
  `AfterTurn`, `SessionEnded`, `HostStopping`.
- Tool observation events bridged from Rig `AgentHook` via `ExtensionHook`:
  `ToolCallEvent` and `ToolResultEvent` (mutable tool-result rendering).
- REPL input surface `on_input` returning `InputAction::{Handled, Transform,
  Continue}`, and in-process `ExtensionCommand` handling.

Current production registrations: none (builder constructs an empty registry);
the surface is exercised by the eval harness (`src/engine/eval/mock.rs`,
`eval/context.rs`) and tests/docs examples. Target: eval-harness hooks move onto
`LifecycleCapability` implementations; the legacy in-process `Extension` trait
and its registration surfaces are deleted (spec REQ-013).

## 5. Skill capabilities (data-only, spec non-negotiable)

Resolution + precedence from `src/skills/mod.rs` (scan/precedence logic stays
host; roots become `SkillCapability` sources):

1. Embedded built-ins (`build.rs` -> `builtin_skills.rs`, addressed `rho://skills/<name>`)
2. User: `<config_dir>/skills/` (`SKILL.md` dirs or flat `.md`)
3. Project: `.rho/skills/`, `prompts/skills/`, `skills/` in the workspace

Later origins replace earlier same-name skills; content is rendered/loaded as
data and never executed. `prompts/skills/` in this repository is a live project
skill root, not a build asset.

## 6. MCP support -> bundled MCP tool-source plugin (in `rho-plugin-builtin`)

From `src/plugin/mcp/` + `load_mcp_capabilities`: `config.mcp.enabled` +
per-server `[mcp.servers.<name>]` entries (enabled flag, spawn command,
working dir) -> spawn -> JSON-RPC 2.0 initialize -> tools advertised as
`McpToolCapability` under their capability ids. MCP-wired tools pass through
the same host floor as any other tool dispatch.

## 7. UI/presentation surfaces -> `rho-plugin-ui`

- Live bottom-screen TUI (`src/ui/interactive/{controller,layout,state,input,
  events,transcript}.rs`, `src/repl/live.rs`): growing editor, one-line footer
  (activity, model, context %, subscription quota/cooldown, queue count),
  animated activity indicator.
- Queue/steering UX: FIFO submission while active, steering vs follow-up
  intents, `Alt+Up` dequeue (semantics stay core; capture is presentation).
- Tool streams: async tool blocks, diffs, streaming tail output, `Ctrl+O`
  global expand, full reflow on resize (`src/ui/{block,stream}.rs`).
- Markdown rendering + syntax highlighting (`src/ui/markdown/*`, syntect).
- Modals: approval prompts, `ask_user` question rendering
  (`src/ui/question.rs`, `src/ui/render/*`).
- Theme (`src/ui/theme.rs`).
- Non-TTY fallback line editor (`src/repl/input_reader.rs`) and one-shot print
  mode; funnels through `TerminalRenderer` / `ToolStreamPort` (the coupling
  surfaces to break in Slices 4/9).

## 8. Host services that are NOT plugin capabilities (stay core)

Recorded so "no unlisted capability" is auditable; these never become plugins:

- Host safety floor: schema validation, `Workspace` containment (protected
  locations, session exclusions), network scope, approval authority,
  repeat-call protection, audit events (`src/tools/policy.rs`,
  `src/tools/approval/`, `src/plugin/safety_floor.rs`)
- `bash_ast` risk classification (policy input, above)
- Session persistence: `SessionManager`, compaction (dedup, sidecars),
  validation, redaction (`src/session/*`)
- Credential custody: `AuthStore` (`auth.json`), OAuth token dirs; only
  scoped credentials ever cross a capability boundary (`src/auth/*`,
  `plugin/contract.rs` scoped types)
- Configuration: typed keys, precedence defaults -> file -> env -> CLI
  (`src/config/*`)
- Error taxonomy (`src/error.rs`)
- Usage/quota/context tracking; per-model context ceilings
  (`src/engine/{quota,metrics,context}.rs`, `src/plugin/provider.rs`)
- Turn orchestration: steering queue (`PendingMessageQueue`, `QueueMode`),
  cancellation, async/parallel tool execution, eval harness
  (`src/engine/runner/*`, `src/engine/eval/*`)
- Skills resolution engine (precedence/scan; roots are capabilities)
- Instruction/system-prompt assembly: global + workspace `AGENTS.md` /
  `CLAUDE.md` / `.cursorrules`, `SYSTEM.md` overrides
  (`src/engine/context.rs`, `prompts/SYSTEM.md`)
- Host CLI subcommands (thin adapters over host services, not agent
  capabilities): `rho login <provider>`, `rho logout <provider>`, `rho auth
  set|remove` (Ollama Cloud key), `rho config [key] [value]`, `rho models`,
  `rho plugin list|inspect|install|remove`, plus flags `--auto-approve`,
  `--resume`, `--provider/--model`, `--max-turns` (`src/cli.rs`,
  `src/config/cli.rs`)

## Verification cross-references

- Providers: `ProviderId::ALL` (12) vs table above.
- Tools: `DECLARATIONS` (10 entries) vs table above.
- Commands: `SLASH_COMMANDS` + match arms vs table above.
- Lifecycle: `LifecycleEvent` variants vs list above.
- The provider parity matrix test
  (`src/plugin/provider.rs::provider_parity_matrix_covers_catalog_auth_context_quota_and_status`)
  guards provider catalog/auth/context/quota/status coverage.

## Target-crate mapping (plan Decisions)

| Inventory section | Target |
| --- | --- |
| Providers + login/logout | `rho-plugin-providers` |
| Tools, commands, skills, MCP | `rho-plugin-builtin` |
| UI/presentation surfaces | `rho-plugin-ui` |
| Host services (section 8) | `rho-core` / `rho-host` / `rho-engine` (per plan crate table) |