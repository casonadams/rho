# Interactive UI & Terminal Styling

`rho` provides a terminal interface featuring inline streaming, an
upward-expanding multiline editor, real-time performance metrics, and a
universal native theme that adopts your terminal's own palette.

---

## The Interactive Editor & Status Footer

When running in an interactive terminal, `rho` displays a pinned two-line
metrics footer at the bottom of the screen:

```text
agent output remains above in normal scrollback
─────────────────────────────────────────────────────────
Write a message here; wrapped lines and explicit
newlines grow the editor upward.
─────────────────────────────────────────────────────────
~/src/github.com/casonadams/rho (main)
↑6.9k ↓514 5.4%/128k @14t/s       qwen3.8:27b-mlx • high
```

Resizing the terminal clears its scrollback and replays the whole session
transcript at the new width, so earlier output reflows instead of being
duplicated. Output printed before `rho` started is cleared along with it.

The input and footer always end on the last row of the terminal. `rho` still
prints inline, not full screen: when little has been printed, blank rows sit
above the input so it stays at the bottom, and as output fills the screen the
input sits directly under the newest line. A menu or a growing editor takes
those blank rows instead of scrolling. When one closes over a full screen, the
rows it covered are printed again so no gap opens above the input (on terminals
that keep a cleared screen in scrollback, those rows then appear there twice).
On start, the visible screen is scrolled into scrollback so `rho` begins on a
blank screen.

- **Top Status Line**: Displays the current working directory, active git
  branch, and session name.
- **Bottom Metrics Line**:
  - `↑`: Tokens sent (prompt + context).
  - `↓`: Tokens received (completion).
  - `R`/`W`: Cached prompt tokens read / written (when supported by provider).
  - `$`: Estimated session cost.
  - `%/window`: Context window utilization percentage and capacity.
  - `@t/s`: Token generation velocity.
  - Model and thinking level aligned to the right.
- **Activity Spinner**: Animates in-place on the top divider of the input frame
  during inference or tool execution without causing scroll jitter or changing
  layout height.

> **Local-Only Metrics**: All status footer counters (tokens, speeds, context
> window usage, costs) are computed strictly in-memory on your local machine.
> `rho` collects no telemetry and transmits no analytics.

### Customizable Prompt, Segmented Status Bar & Per-Turn Telemetry

By default, `rho` renders the two-line footer and plain prompt above. You can opt
into custom prompt chrome, a single-line segmented/powerline status bar, and
per-turn telemetry rows under `[ui.prompt]`, `[ui.status_bar]`, and `[ui.display]`
in `~/.config/rho/config.toml` (or `.rho/config.toml`):

- **`[ui.prompt]`**: Customize the prompt `prefix` glyph (e.g. `"❯ "`, `"» "`),
  toggle `show_version`, set a custom `divider_label` (e.g. `"ϱ"`), and set
  `divider_style` (`"thinking"`, `"dim"`, or `"none"`).
- **`[ui.status_bar]`**: Choose a `preset` (`"minimal"`, `"compact"`, `"full"`,
  `"nerd"`, `"ascii"`, `"custom"`), order `left` / `right` segments (`model`,
  `path`, `git` with `+staged *unstaged ?untracked` counts, `pr`, `context_pct`,
  `context_total`, `token_in`, `token_out`, `token_total`, `cache_read`,
  `cache_write`, `cache_hit`, `token_rate`, `cost`, `time_spent`, `time`,
  `session`, `session_name`, `subagents`, `quota`, `hostname`), pick a
  `separator` (`"powerline"`, `"powerline-thin"`, `"slash"`, `"pipe"`, `"dot"`,
  `"block"`, `"none"`, `"ascii"`), set `icons` (`"nerd"`, `"unicode"`, `"ascii"`), toggle
  `transparent` or `compact_thinking_level`, and optionally enable a
  context-reactive bottom divider (`context_line = "percentage" | "annotated" | "embedded"`).
  Narrow terminals drop lower-priority tail segments whole before truncating.
- **`[ui.display]`**: Enable `show_token_usage = true` to print a dim per-turn
  stats row below each completed assistant turn (timestamp, optional `Δ` turn
  duration with `show_turn_time = true`, input/output/cache tokens, TTFT, and
  right-aligned `tok/s`), and `cache_miss_marker = true` to flag warm prompt
  cache invalidations. See [Configuration](configuration.md) for all keys.

---

## A2UI v1.0 Widgets, Modals & the `ask` Tool

`rho` includes an in-process **A2UI v1.0** (`rho.tui.v1`) declarative surface renderer that maps layout (`Column`, `Row`, `Card`, `Divider`), display (`Text`, `Code`, `Badge`, `Table`), and interactive (`Button`, `ChoicePicker`, `TextField`, `CheckBox`) components directly to `rho`'s ANSI-16 semantic palette (no hardcoded RGB or hex colors).

### Persistent Widget Regions (Opt-In)

Persistent widgets are opt-in and disabled by default (`[ui] a2ui_widgets = false`). Enable them in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[ui]
a2ui_widgets = true

[ui.a2ui]
max_above_lines = 8
max_below_lines = 4
todo_widget = true
subagent_widget = true
decision_assist = false
```

- **Placement & height bounds**: Surfaces with `aboveEditor` placement render above the prompt editor (capped at `max_above_lines`, default `8`); surfaces with `belowEditor` placement render below it (capped at `max_below_lines`, default `4`).
- **`rho:todo` (`aboveEditor`, priority `10`)**: Controlled by `todo_widget` (default `true`). Displays up to 3 active or pending items from the `todo` tool alongside a `done/total` progress badge, and automatically deletes itself when the list is empty or all items are completed.
- **`rho:subagents` (`aboveEditor`, priority `20`)**: Controlled by `subagent_widget` (default `true`). Displays running background subagents with their name, optional role, status badge, and elapsed time, and automatically deletes itself when no subagents are running.

### Interactive A2UI Modals & the `ask` Tool

- **Built-in `ask` tool**: Presents 1 to 5 clarifying questions in a modal A2UI surface (`rho:ask`), each supporting 0 to 4 structured choices (`ChoicePicker` with optional `multi_select`, `recommended` index, and custom `allow_other` input) or free-text input (`TextField`). Prompts and headers are credential-redacted before display.
- **Surface-aware availability**: `ask` is registered only in interactive TUI sessions and is stripped in headless runs (`rho -p`, RPC) and subagents.
- **Optional `DecisionModel` assist**: When `[ui.a2ui] decision_assist = true` (default `false`), questions with 2+ choices and no explicit `recommended` index query `DecisionModel` via the role fallback chain `models.widget` -> `models.judge` -> `models.guard` (600ms timeout, falling back to index `0`) to pre-highlight the most likely default choice.

---

## Keyboard Controls & Navigation

For the comprehensive reference categorized by lifecycle, prompt queueing,
and editor navigation, see
**[Keyboard Shortcuts & Controls](shortcuts.md)**.

| Key                      | Action                                                                                                                |
| :----------------------- | :-------------------------------------------------------------------------------------------------------------------- |
| `Enter`                  | Submit prompt.                                                                                                        |
| `Shift+Enter`            | Insert a newline without submitting.                                                                                  |
| `Ctrl+J`                 | Insert a newline (compatible with terminals encoding it as raw line feed).                                            |
| `Alt+Enter`              | Submit with follow-up queueing (enters FIFO queue).                                                                   |
| `Alt+Up`                 | Dequeue the most recently queued message back into the prompt editor.                                                 |
| `Escape`                 | Interrupt running model generation or tool execution. When idle, clear input draft (double-press opens session tree). |
| `Ctrl+C`                 | Clear the current input prompt draft.                                                                                 |
| `Ctrl+D`                 | Exit `rho` (when prompt is empty).                                                                                    |
| `Ctrl+L`                 | Open interactive model selector modal.                                                                                |
| `Ctrl+P`                 | Cycle to next model.                                                                                                  |
| `Shift+Ctrl+P` / `Alt+P` | Cycle to previous model.                                                                                              |
| `Shift+Tab`              | Cycle thinking / reasoning effort level.                                                                              |
| `Ctrl+T`                 | Toggle thinking blocks visibility (visible / hidden).                                                                 |
| `Ctrl+O`                 | Toggle expand/collapse of tool output cards in the transcript.                                                        |
| `Ctrl+V`                 | Paste image from clipboard into the session.                                                                          |
| `Ctrl+X`                 | Copy last assistant message to clipboard.                                                                             |
| `Ctrl+G`                 | Open current input draft in external editor (`$EDITOR`).                                                              |
| `Ctrl+Z`                 | Suspend process to background.                                                                                        |
| `Tab`                    | Auto-complete slash commands, skill names, and file paths.                                                            |
| `Left` (empty prompt)    | Open the Agents list; `Enter` shows a subagent in the main terminal in place of the main agent, and `Left` on an empty prompt returns. See [Workflows](workflows.md#watching-agents). |

### Message Queueing

Messages submitted while the agent is busy are placed in an in-memory FIFO
queue. Once the current turn settles, queued messages execute sequentially
without losing context.

A steering message (`Enter` while the agent runs) waits in the queue until the
agent reaches its next tool boundary, or until it would otherwise finish its
answer (then it keeps working with your message, unless it is on its last
turn). When the agent takes it, it leaves the queue and appears in the
transcript as your message, at the point where the agent read it.

---

## REPL Slash Commands

Type `/` in the editor to access built-in commands (press `Tab` to
autocomplete):

| Command                     | Description                                                                                               |
| :-------------------------- | :-------------------------------------------------------------------------------------------------------- |
| `/help [command]`           | Open interactive command reference modal, or display detailed usage and subcommands for a command. Works mid-turn. |
| `/model [<provider>/<model>]` | Open interactive model selector modal, or switch model directly. Works mid-turn. |
| `/models`                   | Open interactive role-model configuration modal (`default`, built-in roles like `smol`, `slow`, `guard`, `judge`, `widget`, and custom roles). Works mid-turn. |
| `/thinking [level]`         | Open interactive thinking selector modal, or set reasoning effort directly (`auto`, `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`; alias: `/think`). Default `medium`; subagents follow the current level. Works mid-turn. A change while the prompt cache is warm shows a notice that the next request rewrites the cache. |
| `/settings`                 | Open interactive runtime settings modal (block style, response framing, model, guard model, thinking, output toggles, and Tools & Permissions). Works mid-turn. |
| `/tools`                    | Open interactive Tools & Permissions modal (alias: `/permissions`). Works mid-turn. |
| `/agents`                   | Open the subagent monitor modal (same as pressing `Left` on an empty prompt). Works mid-turn. |
| `/btw [question]`           | Open an ephemeral side-question overlay using the current session context without mutating history. Works mid-turn. |
| `/recap`                    | Generate a concise one-line `※ recap: ...` summary of the current session without mutating history. Works mid-turn. |
| `/resume [id]`              | Open session selector (rows show name or first message, age, turns, ID), or resume a session by ID. In the selector, `Ctrl+D` then `Ctrl+D` again deletes the highlighted session; any other key cancels the delete. |
| `/session`                  | Display token capacity, cost, context window utilization, and diagnostics (alias: `/tokens`).             |
| `/compact [instructions]`   | Summarize earlier context to reclaim context window space.                                                |
| `/tree`                     | Open interactive conversation turn and branch DAG tree viewer.                                            |
| `/rewind <turn>`            | Rewind context to a specific prior turn in the active branch.                                             |
| `/fork [pane\|hsplit\|vsplit\|window\|tab] [turn\|id]` | Fork session from a turn or node; optional target opens it in a new multiplexer/terminal pane, window, or tab (safe mid-turn). |
| `/clone`                    | Duplicate active branch into a new session.                                                               |
| `/name [name]`              | Name the current session; the name titles it in `/resume`.                                                |
| `/clear`                    | Start a fresh session while preserving session history on disk (alias: `/new`).                           |
| `/skill [name]`             | List configured skills, or invoke a declarative workflow (e.g. `/skill:plan`).                            |
| `/mcp`                      | Open the interactive Model Context Protocol modal (toggle servers, inspect status).                       |
| `/mcp login\|logout <name>` | Sign in to or out of a remote MCP server that uses OAuth (see [MCP](mcp-and-hooks.md#remote-servers-with-oauth)). |
| `/mcp prompts [server]`     | List MCP server prompts and their arguments (see [Prompts and Resources](mcp-and-hooks.md#prompts-and-resources)). |
| `/mcp prompt <server> <name> [key=value ...]` | Fetch an MCP server prompt and send it as your next prompt. |
| `/login [provider]`         | Authenticate with an AI provider (OAuth PKCE or API key).                                                 |
| `/logout [provider]`        | Remove stored credentials for a provider.                                                                 |
| `/reload`                   | Reload configuration, skills, and MCP tools without losing conversation history.                          |
| `/export [html\|md] [path]` | Export the active session branch as a clean Markdown or HTML document.                                    |
| `/workflow`                 | List available [workflows](workflows.md) with their inputs, plus any invalid workflow files.              |
| `/workflow <name> [inputs]` | Run a workflow, e.g. `/workflow refine security`. Arguments fill inputs in order; `name=value` sets one by name. |
| `/exit`                     | Exit rho (alias: `/quit`).                                                                                |

Background work (subagents, which start in the background unless
`subagents.foreground_seconds` is set, and `bash`
commands that move to the background) belongs to the session that started it.
A turn waits for, and is told about, only its own session's background jobs.
Likewise a subagent, including a workflow agent, owns the background commands
it starts: its turn waits for them, and they stop when its round ends. Where no
agent could receive a command's output, `bash` never moves it to the
background: `background: true` is refused and a timeout stops the command.
Switching to another session with `/resume` or `/clear` stops the old session's
background work and drops its undelivered results, because nothing could deliver
them. Pressing `Esc` to interrupt a turn also stops the session's running
background work. Reloading (`/reload`, `/model`, `/login`) keeps the same session and its
background work running.

### Ephemeral Side Questions (`/btw`)

Run `/btw [question]` at any time, including mid-turn, to open the `Side Question (/btw)` overlay modal without interrupting the active turn or adding to canonical conversation history.

- **Context & streaming**: Answers use the current session context (including any partial assistant response when invoked mid-turn) and stream live into the modal.
- **Follow-ups & navigation**: Type follow-up questions in the `btw> ` prompt (`Shift+Enter` or `Ctrl+J` inserts a newline; `Up`/`Down` and `PageUp`/`PageDown` scroll the response).
- **Clipboard & dismiss**: Press `Ctrl+Y` to copy the latest `/btw` answer to the system clipboard, or `Esc` to dismiss the overlay and cancel any in-flight stream.

### Idle & Resume Session Recap (`/recap`)

Run `/recap` (works mid-turn) to generate a concise one-line `※ recap: ...` notice summarizing what the session was doing, without mutating conversation history. Running `/recap` again updates the trailing recap line in place rather than duplicating it.

Opt-in automatic recaps can be enabled under `[recap]` in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[recap]
enabled = false      # Trigger a recap after idle_seconds on an empty prompt (default: false)
idle_seconds = 240   # Inactivity delay before idle recap runs, clamped to 1..=3600 (default: 240)
on_resume = false    # Trigger a recap when resuming a non-empty session (default: false)
```

Idle recaps only arm when transcript history exists and the prompt editor is empty, and are cancelled immediately if you start typing. See [Configuration](configuration.md#session-recap) for `rho config` commands.

### Adaptive Thinking Effort (`/thinking auto`)

Selecting `/thinking auto` (or setting `thinking_level = "auto"` in `config.toml`) enables per-turn effort classification instead of a fixed reasoning level. Before each turn, `rho` classifies the prompt using `models.judge` (falling back to `models.guard` when `models.judge` is not configured) and selects the thinking effort for that turn.

---

## Context Management

`rho` keeps long sessions inside the model's context window in two stages.

- **Tool-output pruning (every turn).** Before each request, older tool results
  longer than 15 lines are replaced with a short stub that keeps the tool,
  target, line count, and size. Bash stubs keep the exit code and log path.
  `read`, `rg`, and `fd` stubs keep `full: <path>` when the original
  output was spilled to `.rho/artifacts`. Results from the latest turn and the
  most recent tokens (scaled to the model's context window) are never pruned.
- **Prompt-cache stability.** The pruning boundary moves in steps rather than
  every turn, so the pruned prefix stays byte-identical between steps and
  provider prompt caches keep hitting. While the cache from the previous
  request is still alive (within `cache_retention`), rho resends exactly the
  stubs it sent last time and prunes further only when the saving is at least
  a quarter of the history it would have to re-cache.
- **Auto-compaction.** When the history approaches the context limit minus the
  reserve (default 16,384 tokens), earlier turns are summarized: before a turn
  starts (the prompt is then sent with the summary), and during a turn as soon
  as the next request's prompt would pass that point (the last prompt plus its
  reply and the tool results added since), so the turn compacts and carries on
  with its saved work instead of waiting for the provider to reject a request.
  A request rejected as too long is compacted and retried the same way. A turn
  compacts at most three times; after that, or when an early compaction finds
  nothing to fold, it carries on until the window is actually full, and an
  overflow then ends the turn. Subagents follow the
  same rule against their own model's window. A `todo` list
  with unfinished items is copied into the summary as a `<todos>` block, so the
  agent keeps its plan. After pruning
  changes the history, the size check estimates the pruned history instead of
  reusing the previous request's reported usage. Use `/compact` to run it
  manually.
- **Context budget notices.** The first time a request's prompt passes 50%,
  75%, or 90% of the context window, the next tool results sent to the model
  end with one line such as
  `[context: 142k of 200k tokens used (71%); compaction starts near 184k]`, so it
  can delegate to subagents or read less. Each percentage is reported once
  until the next compaction, branch switch, or rewind, and no notice is sent
  while the window size is only an estimate. The line goes only on the newest tool results (a
  turn that ends without tool calls reports it on a later one), so the cached
  prompt prefix never changes, and it is saved with that tool result.
  Subagents receive their own notices against their model's window.
- **Context window source.** `context_limit` (or `AI_CONTEXT_LIMIT`) always
  wins, for subagents' models too. Otherwise rho uses the window the provider lists for the model in its
  live model list (fetched each session, never cached on disk). For local
  Ollama, that is the window the server actually allocated (read from `/api/ps`
  after each turn), falling back to the model's `num_ctx` or trained maximum
  until the model has been loaded. Providers that list no window (the OpenAI
  API, most custom endpoints) use a built-in estimate. `/session` shows which
  source applies (`from provider`, `configured`, or `estimated`). If the Ollama
  check cannot reach the server, `/session` shows
  `Context Window Check: failed: <reason>` until the next check succeeds.

---

## Theming & Terminal Styling

`rho` ships a single universal theme built from your terminal's own 16-color
ANSI palette: cyan accents, blue tool headers, green success, red errors, yellow
warnings, and magenta skill tags. There is no theme configuration; the interface
always matches your terminal emulator, in dark or light mode.

- Thinking output and assistant replies share a one-column left margin. Thinking
  is rendered as markdown in the dim style, and the live stream and the
  committed block use the same renderer, so a redraw never changes its layout.
  The input area uses the same one-column indent.
- Every finished tool card ends with a dim `Took 6.054s` line on its own line.
  While a tool runs, its live card is the same card and counts `Elapsed ...` up.
  A running `bash` card shows its output tidied like the finished one (surrounding
  blank lines dropped, runs of identical lines folded), so it does not resize
  when the command ends.
  Streamed output (reply text, thinking and live tool output) that the API
  delivers in bursts is spread over the pause before the next burst, a few
  words or lines per frame and never longer than 600 ms behind; a steady stream
  is shown as it arrives.
  `web_fetch` shows the content kind inline and dim, such as
  `web_fetch https://example.com (text)`. `todo` shows the list's progress and
  the item in progress, such as `todo 1/3 done · Add tests`; `Ctrl+O` shows the
  whole list. `undo` shows the file path, `lsp` the action with `path:line`
  and symbol, `memory` the action and scope, and `read` with a `symbol` shows
  `path symbol=name`.
- Edit cards show `N edits · +added -removed` in the header and label every
  edit in a multi-edit call (`@@ edit #1/3 · line 12 @@`). Write cards show
  line count, size, and whether the file was new or overwritten. Line numbers
  and the new/overwrite state are captured before the tool runs, so redraws
  never re-read the file. A failed edit also shows the change it attempted.
- While the model is still writing a `write` call, its live card appears as
  soon as the path is known and tails the file content as it streams, like
  `bash` output. An `edit` call's card opens as soon as the model starts it and
  grows its diff a line at a time as the old and new text stream in; it stays
  up while the edit runs, line numbers and all, until the finished card
  replaces it. The reply text before any tool call is shown in full as soon
  as the model starts the call (Claude, OpenAI-compatible, and ChatGPT
  providers stream tool arguments; others show the call once it is complete).
  On Claude, rho sets `eager_input_streaming` on the `write` and `edit` tools
  so the API sends their arguments as they are generated instead of all at once.
  Both tools list `path` first, so a live edit card numbers its lines while the
  edits stream.
- Collapsed cards show their last lines under a notice row: tool output keeps
  five rows under `... (N earlier lines, Ctrl+O to expand)`, and edit diffs and
  write previews grow to ten rows under
  `... (N earlier lines, M total, Ctrl+O to expand)`, then keep that height while
  streaming. `Ctrl+O` shows everything. Tabs render as four spaces.
- Notices carry a bold `info`, `warning`, or `error` label.
- Secondary text (thinking blocks, footer telemetry, hints, diff context, and
  table borders) uses the terminal's native dim (SGR 2) effect instead of a
  fixed palette color, so it stays readable on any palette.
- Tool cards, user messages, and notices keep their container block framing. By
  default, `rho` uses a subtle solid container fill derived from your terminal
  palette. You can replace the solid background with an outline/border and
  customize border colors via the `[ui]` section in `~/.config/rho/config.toml`.
- Code blocks are syntax highlighted and reduced to native ANSI-16 colors.

### Block Styling & Outline Borders

`rho` defaults to outline borders (`block_style = "border"`). You can switch
between outline borders and solid background fills, configure thinking and tool
output defaults, or adjust borders in `~/.config/rho/config.toml` (or
interactively via `/settings`, which automatically persists your changes):

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
agent_border = "gray"          # Agent / sub-agent skill blocks
tool_border = "gray"           # General command / tool cards
bash_success_border = "gray"   # Successful bash commands
bash_error_border = "red"      # Failed bash commands
```

Border mode renders clean rounded box-drawing characters (`╭`, `─`, `╮`, `│`,
`╯`, `╰`) in the chosen colors, eliminating solid background rectangles while
maintaining visual distinction between user inputs, commands, and success/fail
statuses.

### Pairing with `walh-shell`

Because `rho` emits only native 16-color ANSI escape codes, pairing with
[walh-shell](https://github.com/casonadams/walh-shell) mirrors the shell's
active wallpaper palette and surfaces without any configuration.

---

## Terminal Multiplexers (tmux, zellij)

When running `rho` in a multi-pane terminal multiplexer (such as `tmux`)
alongside another active pane, background activity (e.g., live-streaming tokens
or the turn spinner) can cause the hardware cursor in the active pane to flicker
if the multiplexer does not forward synchronized rendering to the outer terminal
emulator.

`rho` wraps live diffs in Mode 2026 synchronized update escape sequences
(`\x1b[?2026h` / `\x1b[?2026l`). To ensure `tmux` buffers pane repaints and
keeps the hardware cursor anchored in the active pane, enable `sync` and focus
reporting in `~/.tmux.conf`:

```tmux
# Pass synchronized output to outer terminals supporting Mode 2026 (Alacritty, Kitty, iTerm2, Ghostty)
set -as terminal-features ',*:sync'

# Forward focus events so background panes can suppress inactive cursor states
set -g focus-events on
```

### Native Hardware Cursor vs. Software Cursor

`rho` defaults to the native hardware cursor (`cursor = "hardware"`). This
preserves your terminal emulator's native cursor shape (beam `|`, underline `_`,
or block `█`), custom colors, blink preferences, screen-reader accessibility,
and IME candidate popup positioning across modern terminals (Alacritty, Kitty,
Ghostty, WezTerm, iTerm2, and Windows Terminal).

If you prefer a retro reverse-video block cursor or are running in an
environment that does not support synchronized updates, you can switch to
software cursor mode:

```toml
[ui]
cursor = "software"
```

(or toggle it interactively in `/settings` under **Cursor Style**).

---

## Runtime Settings Modal (`/settings`)

The interactive `/settings` modal allows reviewing and tweaking runtime preferences without exiting or restarting the session:

- **Block Style**: Switch between `Border` (outline) and `Solid` (fill) container styling.
- **Box Responses**: Toggle boxed frames for agent transcript responses (`On` / `Off`).
- **Thinking Effort**: Cycle through reasoning effort tiers (`auto`, `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`).
- **Thinking Output**: Toggle thinking trace visibility in the transcript (`Shown` / `Hidden`).
- **Tool Output**: Toggle default tool card expansion (`Expanded` / `Collapsed`).
- **Version Banner**: Toggle version banner visibility at the start of transcripts (`Shown` / `Hidden`).
- **Cursor Style**: Switch between `Hardware` and `Software` prompt cursors.
- **Models**: Displays the active model with a drill-down indicator (e.g. `claude-3-7-sonnet  ›`). Pressing `Enter` opens the **Models** submenu to configure `Default`, built-in roles (`Smol`, `Slow`, `Guard`, `Judge`, `Widget`, `Review`, `Plan`, `Task`, `Vision`), and any configured custom roles. Selecting a model updates the setting in-place across all menu levels while keeping you inside the menu.
- **Tools & Permissions**: Interactive submenu (`Configure  ›`) to manage tool availability and search providers.

Submenus and child pickers maintain a true modal stack: selecting an option updates the setting dynamically in-place, while `Esc` navigates back one level (`Select Model` → `Models` → `Settings` → REPL prompt).

---

## Tools & Permissions Modal

Inside `/settings`, selecting **Tools & Permissions** opens an interactive submenu to manage tool availability and search providers:

- **Search Engine**: Displays the current primary search provider (e.g. `brave`, `duckduckgo`, `exa`). Pressing `Enter` opens the **Select Search Engine** modal listing all supported engines (`brave`, `duckduckgo`, `yahoo`, `exa`) with aligned descriptions, API key requirements, and active checkmarks (`✓`). Selecting an engine updates runtime state and persists to `[tools.web.search] default`.
- **Web Search**: Interactive `On` / `Off` toggle for the built-in `web_search` tool (`[tools.web.search] enabled`). When disabled, `web_search` is excluded from registration and model prompts.
- **Web Fetch**: Interactive `On` / `Off` toggle for the built-in `web_fetch` tool (`[tools.web.fetch] enabled`).
- **MCP**: Interactive `On` / `Off` toggle for the Model Context Protocol subsystem (`[mcp] enabled`).
- **Permissions**: Interactive `On` / `Off` toggle for runtime mutation guardrails (`[permission] enabled`).

