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
- **Activity Spinner**: Animates in-place on the status line during inference or
  tool execution without causing scroll jitter.

> **Local-Only Metrics**: All status footer counters (tokens, speeds, context
> window usage, costs) are computed strictly in-memory on your local machine.
> `rho` collects no telemetry and transmits no analytics.

---

## Keyboard Controls & Navigation

For the comprehensive reference categorized by lifecycle, prompt queueing,
editor navigation, and custom keybinding configuration, see
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

### Custom Keybindings

Keybindings can be customized by adding a `keybindings.toml` (or
`keybindings.json`) to `~/.config/rho/`:

```toml
[bindings]
"app.interrupt" = ["escape"]
"app.clear" = ["ctrl+c"]
"app.model.select" = ["ctrl+l"]
"app.thinking.cycle" = ["shift+tab"]
```

### Message Queueing

Messages submitted while the agent is busy are placed in an in-memory FIFO
queue. Once the current turn settles, queued messages execute sequentially
without losing context.

---

## REPL Slash Commands

Type `/` in the editor to access built-in commands (press `Tab` to
autocomplete):

| Command                     | Description                                                                                                       |
| :-------------------------- | :---------------------------------------------------------------------------------------------------------------- |
| `/help`                     | Display command summary and keyboard shortcuts.                                                                   |
| `/model [model] [provider]` | Open interactive model selector modal, or switch model/provider directly.                                         |
| `/thinking [level]`         | Configure thinking effort (`off`, `minimal`, `low`, `medium`, `high`, `max`), or open selector (alias: `/think`). |
| `/settings`                 | Open interactive runtime settings modal (display toggles, auto-compact, etc.).                                    |
| `/resume [id]`              | Open interactive session selector modal, or resume a prior session by ID.                                         |
| `/session`                  | Display token capacity, cost, context window utilization, and diagnostics.                                        |
| `/compact [instructions]`   | Summarize earlier context to reclaim context window space.                                                        |
| `/tree`                     | Open interactive conversation turn and branch DAG tree viewer.                                                    |
| `/rewind <turn>`            | Rewind context to a specific prior turn in the active branch.                                                     |
| `/fork [turn\|id]`          | Fork session from a turn or node into a new session branch.                                                       |
| `/clone`                    | Duplicate active branch into a new session.                                                                       |
| `/name [name]`              | Assign a human-readable name to the current session.                                                              |
| `/clear`                    | Start a fresh session while preserving session history on disk (alias: `/new`).                                   |
| `/skill [name]`             | List configured skills, or invoke a declarative workflow (e.g. `/skill:plan`).                                    |
| `/plugin`                   | List configured MCP tool servers and lifecycle plugins.                                                           |
| `/mcp`                      | Open the interactive Model Context Protocol modal (toggle servers, inspect status).                               |
| `/login [provider]`         | Authenticate with an AI provider (OAuth PKCE or API key).                                                         |
| `/logout [provider]`        | Remove stored credentials for a provider.                                                                         |
| `/reload`                   | Reload configuration, skills, and MCP tools without losing conversation history.                                  |
| `/export [html\|md] [path]` | Export the active session branch as a clean Markdown or HTML document.                                            |
| `/exit`                     | Exit rho (alias: `/quit`).                                                                                        |

---

## Theming & Terminal Styling

`rho` ships a single universal theme built from your terminal's own 16-color
ANSI palette: cyan accents, blue tool headers, green success, red errors,
yellow warnings, and magenta skill tags. There is no theme configuration; the
interface always matches your terminal emulator, in dark or light mode.

- Secondary text (thinking blocks, footer telemetry, hints, diff context, and
  table borders) uses the terminal's native dim (SGR 2) effect instead of a
  fixed palette color, so it stays readable on any palette.
- Tool cards, user messages, and notices keep their container block framing.
  By default, `rho` uses a subtle solid container fill derived from your terminal palette.
  You can replace the solid background with an outline/border and customize border colors
  via the `[ui]` section in `~/.config/rho/config.toml`.
- Code blocks are syntax highlighted and reduced to native ANSI-16 colors.
- Diagrams use your terminal's ANSI foreground.

### Block Styling & Outline Borders

To replace the solid background fill on blocks with an outline/border, configure
`block_style = "border"` in `~/.config/rho/config.toml` (or project `.rho/config.toml`):

```toml
[ui]
# Block framing: "border" (outline) or "solid" (fill, default)
block_style = "border"

# Border colors (ANSI color names or "#rrggbb" hex; defaults to "gray")
user_border = "gray"           # User prompt blocks
agent_border = "gray"          # Agent / sub-agent skill blocks
tool_border = "gray"           # General command / tool cards
bash_success_border = "gray"   # Successful bash commands
bash_error_border = "red"      # Failed bash commands
```

Border mode renders clean rounded box-drawing characters (`╭`, `─`, `╮`, `│`, `╯`, `╰`)
in the chosen colors, eliminating solid background rectangles while maintaining visual
distinction between user inputs, commands, and success/fail statuses.

You can also toggle between `border` and `solid` block styles dynamically at any time
by pressing `Ctrl+S` (action `app.blockStyle.toggle`). The active preference will
automatically persist to your configuration file.

### Pairing with `walh-shell`

Because `rho` emits only native 16-color ANSI escape codes, pairing with
[walh-shell](https://github.com/casonadams/walh-shell) mirrors the shell's
active wallpaper palette and surfaces without any configuration.

### Mermaid Diagram Rendering

Fenced Mermaid diagram blocks (` ```mermaid `) render in the terminal as
clean, compact Unicode box-drawing diagrams.

- **Flowcharts (`graph TD` / `LR`):** Rendered natively with compact per-node
  box sizing, true vertical top-down flow (`TD`/`TB`), and collision-free
  perimeter routing for cycles and feedback loops.
- **Decision Shapes:** Supports rectangles `[label]`, rounded boxes `(label)`,
  and decision diamonds `{choice}`.
- **Clean Display:** Rendered diagrams display directly without surrounding
  fences. If a diagram contains parsing errors, it falls back to a clean
  code block.
- **Viewport Fitting:** Diagrams wider than your terminal viewport are safely
  clipped to the right margin to preserve box alignment. Keep horizontal
  chains concise or use `TD` for vertical stacking.
