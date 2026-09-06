# Interactive UI & Theming

`rho` provides a terminal interface featuring inline streaming, an
upward-expanding multiline editor, real-time performance telemetry, and
customizable themes.

---

## The Interactive Editor & Status Footer

When running in an interactive terminal, `rho` displays a pinned two-line
telemetry footer at the bottom of the screen:

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
- **Bottom Telemetry Line**:
  - `↑`: Tokens sent (prompt + context).
  - `↓`: Tokens received (completion).
  - `R`/`W`: Cached prompt tokens read / written (when supported by provider).
  - `$`: Estimated session cost.
  - `%/window`: Context window utilization percentage and capacity.
  - `@t/s`: Token generation velocity.
  - Model and thinking level aligned to the right.
- **Activity Spinner**: Animates in-place on the status line during inference or
  tool execution without causing scroll jitter.

---

## Keyboard Controls & Navigation

| Key           | Action                                                                     |
| :------------ | :------------------------------------------------------------------------- |
| `Enter`       | Submit prompt.                                                             |
| `Shift+Enter` | Insert a newline without submitting.                                       |
| `Ctrl+J`      | Insert a newline (compatible with terminals encoding it as raw line feed). |
| `Alt+Enter`   | Submit with follow-up queueing (enters FIFO queue).                        |
| `Ctrl+O`      | Toggle expand/collapse of tool output cards in the transcript.             |
| `Ctrl+C`      | Interrupt running model generation or tool execution.                      |
| `Escape`      | Clear idle input draft, or cancel active execution and restore queue.      |

### Message Queueing

Messages submitted while the agent is busy are placed in an in-memory FIFO
queue. Once the current turn settles, queued messages execute sequentially
without losing context.

---

## REPL Slash Commands

Type `/` in the editor to access built-in commands:

| Command                     | Description                                                                      |
| :-------------------------- | :------------------------------------------------------------------------------- |
| `/theme`                    | Open interactive theme picker with live preview.                                 |
| `/model [name]`             | Switch the active model or provider in place.                                    |
| `/reload`                   | Reload configuration, skills, and MCP tools without losing conversation history. |
| `/export [html\|md] [path]` | Export the active session branch as a clean Markdown or HTML document.           |
| `/skill:<name>`             | Invoke a declarative skill workflow.                                             |
| `/help`                     | Display command summary and keyboard shortcuts.                                  |

---

## Theming & Terminal Styling

`rho` includes 10 built-in themes (9 dark palettes, plus `catppuccin-latte` for
light backgrounds).

Run `/theme` to launch the theme selector with immediate live preview. Selection
is saved to `~/.config/rho/config.toml`.

### Custom Themes

Create custom themes by adding TOML files to
`~/.config/rho/themes/<theme_name>.toml`:

```toml
name = "my-custom-theme"
background = "#181825"
foreground = "#cdd6f4"

# 16-color ANSI palette mapping
color0  = "#11111b"
color1  = "#f38ba8"
color2  = "#a6e3a1"
color3  = "#f9e2af"
color4  = "#89b4fa"
color5  = "#f5c2e7"
color6  = "#94e2d5"
color7  = "#bac2de"
color8  = "#585b70"
color9  = "#f38ba8"
color10 = "#a6e3a1"
color11 = "#f9e2af"
color12 = "#89b4fa"
color13 = "#f5c2e7"
color14 = "#94e2d5"
color15 = "#a6adc8"
```

### Pairing with `walh-shell`

The built-in `default` theme avoids hardcoded hex colors and emits native
16-color ANSI escape codes. When paired with
[walh-shell](https://github.com/casonadams/walh-shell), `rho` dynamically
mirrors the shell's active wallpaper palette and surfaces.

### Mermaid Diagram Rendering

Fenced Mermaid diagram blocks (` ```mermaid `) render in the terminal as
monochrome diagrams. For best readability:

- With the `default` theme, diagrams use your terminal's ANSI foreground.
- Diagrams wider than your terminal viewport are safely clipped to the right
  margin to preserve box alignment; keep diagrams narrow or split complex
  topologies into multiple blocks.
