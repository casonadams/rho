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

### Message Queueing

Messages submitted while the agent is busy are placed in an in-memory FIFO
queue. Once the current turn settles, queued messages execute sequentially
without losing context.

---

## REPL Slash Commands

Type `/` in the editor to access built-in commands (press `Tab` to
autocomplete):

| Command                     | Description                                                                                               |
| :-------------------------- | :-------------------------------------------------------------------------------------------------------- |
| `/help`                     | Open interactive command reference modal (or display summary in non-interactive mode).                    |
| `/model [<provider>/<model>]` | Open interactive model selector modal, or switch model directly.                                 |
| `/thinking [level]`         | Open interactive thinking selector modal, or set reasoning effort directly (alias: `/think`).             |
| `/settings`                 | Open interactive runtime settings modal (block style, response framing, model, guard model, thinking, output toggles, and Tools & Permissions). |
| `/resume [id]`              | Open session selector (rows show name or first message, age, turns, ID), or resume a session by ID.      |
| `/session`                  | Display token capacity, cost, context window utilization, and diagnostics (alias: `/tokens`).             |
| `/compact [instructions]`   | Summarize earlier context to reclaim context window space.                                                |
| `/tree`                     | Open interactive conversation turn and branch DAG tree viewer.                                            |
| `/rewind <turn>`            | Rewind context to a specific prior turn in the active branch.                                             |
| `/fork [turn\|id]`          | Fork session from a turn or node into a new session branch.                                               |
| `/clone`                    | Duplicate active branch into a new session.                                                               |
| `/name [name]`              | Name the current session; the name titles it in `/resume`.                                                |
| `/clear`                    | Start a fresh session while preserving session history on disk (alias: `/new`).                           |
| `/skill [name]`             | List configured skills, or invoke a declarative workflow (e.g. `/skill:plan`).                            |
| `/mcp`                      | Open the interactive Model Context Protocol modal (toggle servers, inspect status).                       |
| `/login [provider]`         | Authenticate with an AI provider (OAuth PKCE or API key).                                                 |
| `/logout [provider]`        | Remove stored credentials for a provider.                                                                 |
| `/reload`                   | Reload configuration, skills, and MCP tools without losing conversation history.                          |
| `/export [html\|md] [path]` | Export the active session branch as a clean Markdown or HTML document.                                    |
| `/exit`                     | Exit rho (alias: `/quit`).                                                                                |

---

## Context Management

`rho` keeps long sessions inside the model's context window in two stages.

- **Tool-output pruning (every turn).** Before each request, older tool results
  longer than 15 lines are replaced with a short stub that keeps the tool,
  target, line count, and size. Bash stubs keep the exit code and log path.
  `read`, `rg`, and `fd` stubs keep `Full output: <path>` when the original
  output was spilled to `.rho/artifacts`. Results from the latest turn and the
  most recent tokens (scaled to the model's context window) are never pruned.
- **Prompt-cache stability.** The pruning boundary moves in steps rather than
  every turn, so the pruned prefix stays byte-identical between steps and
  provider prompt caches keep hitting.
- **Auto-compaction.** When the history approaches the context limit minus the
  reserve (default 16,384 tokens), earlier turns are summarized. After pruning
  changes the history, the size check estimates the pruned history instead of
  reusing the previous request's reported usage. Use `/compact` to run it
  manually.

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
  While a tool runs, its live card shows `Elapsed ...` in the same place.
  `web_fetch` shows the content kind inline and dim, such as
  `web_fetch https://example.com (text)`.
- Edit cards show `N edits · +added -removed` in the header and label every
  edit in a multi-edit call (`@@ edit #1/3 · line 12 @@`). Write cards show
  line count, size, and whether the file was new or overwritten. Line numbers
  and the new/overwrite state are captured before the tool runs, so redraws
  never re-read the file. A failed edit also shows the change it attempted.
- Long edit diffs, write previews, and tool output end with
  `... (N more lines, Ctrl+O to expand)`; `Ctrl+O` shows everything. Tabs render
  as four spaces.
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
- **Thinking Effort**: Cycle through reasoning effort tiers (`off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`).
- **Thinking Output**: Toggle thinking trace visibility in the transcript (`Shown` / `Hidden`).
- **Tool Output**: Toggle default tool card expansion (`Expanded` / `Collapsed`).
- **Version Banner**: Toggle version banner visibility at the start of transcripts (`Shown` / `Hidden`).
- **Cursor Style**: Switch between `Hardware` and `Software` prompt cursors.
- **Models**: Displays the active model with a drill-down indicator (e.g. `claude-3-7-sonnet  ›`). Pressing `Enter` opens the **Models** submenu to configure primary and specialized role models (`Default`, `Smol`, `Slow`, `Guard`). Selecting a model updates the setting in-place across all menu levels while keeping you inside the menu.
- **Tools & Permissions**: Interactive submenu (`Configure  ›`) to manage tool availability and search providers.

Submenus and child pickers maintain a true modal stack: selecting an option updates the setting dynamically in-place, while `Esc` navigates back one level (`Select Model` → `Models` → `Settings` → REPL prompt).

---

## Tools & Permissions Modal

Inside `/settings`, selecting **Tools & Permissions** opens an interactive submenu to manage tool availability and search providers:

- **Search Engine**: Displays the current primary search provider (e.g. `brave`, `duckduckgo`). Pressing `Enter` opens the **Select Search Engine** modal listing all supported engines (`brave`, `duckduckgo`, `yahoo`) with aligned descriptions, API key requirements, and active checkmarks (`✓`). Selecting an engine updates runtime state and persists to `[tools.web.search] default`.
- **Web Search**: Interactive `On` / `Off` toggle for the built-in `web_search` tool (`[tools.web.search] enabled`). When disabled, `web_search` is excluded from registration and model prompts.
- **Web Fetch**: Interactive `On` / `Off` toggle for the built-in `web_fetch` tool (`[tools.web.fetch] enabled`).
- **MCP**: Interactive `On` / `Off` toggle for the Model Context Protocol subsystem (`[mcp] enabled`).
- **Permissions**: Interactive `On` / `Off` toggle for runtime mutation guardrails (`[permission] enabled`).

