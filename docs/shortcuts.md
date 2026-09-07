# Keyboard Shortcuts & Controls

`rho` features an interactive terminal interface with ergonomic keybindings for
prompt editing, model switching, turn cancellation, modal navigation, and
transcript inspection.

---

## Execution & Session Lifecycle

Controls for interrupting runs, managing prompt drafts, and exiting:

| Shortcut | Action                    | Details                                                                                                                                                                  |
| :------- | :------------------------ | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Escape` | **Interrupt / Cancel**    | Cancels active model generation or tool execution. When idle, clears the prompt draft. Pressing `Escape` twice rapidly while the prompt is empty opens the session tree. |
| `Ctrl+C` | **Clear Prompt Draft**    | Clears the current prompt editor draft (both when idle and during turn execution). Does not stop running generation or exit the agent.                                   |
| `Ctrl+D` | **Exit `rho`**            | Closes the REPL session when the prompt editor is empty (standard EOF behavior).                                                                                         |
| `Ctrl+Z` | **Suspend to Background** | Sends `SIGTSTP` to suspend `rho` to the background. Resume with `fg`.                                                                                                    |
| `Ctrl+G` | **External Editor**       | Opens the current multiline input draft in your configured `$EDITOR` (or `$VISUAL`).                                                                                     |

---

## Prompt Submission & Queue Management

Controls for submitting prompts, adding newlines, and managing background turn
queues:

| Shortcut                | Action                                   | Details                                                                              |
| :---------------------- | :--------------------------------------- | :----------------------------------------------------------------------------------- |
| `Enter`                 | **Submit Prompt**                        | Submits the prompt to the active agent turn.                                         |
| `Shift+Enter`           | **Insert Newline**                       | Inserts a newline into the editor without submitting.                                |
| `Ctrl+J` / `Ctrl+Enter` | **Insert Newline (Terminal-compatible)** | Inserts a newline for terminals that map Enter combinations to LF.                   |
| `Alt+Enter`             | **Queue Follow-Up**                      | Submits the message into the FIFO queue without interrupting current execution.      |
| `Alt+Up`                | **Dequeue Message**                      | Pulls the most recently queued message back out of the queue into the prompt editor. |

---

## Model & Reasoning Controls

Quick shortcuts to adjust providers, models, and reasoning effort:

| Shortcut                 | Action                         | Details                                                                                 |
| :----------------------- | :----------------------------- | :-------------------------------------------------------------------------------------- |
| `Ctrl+L`                 | **Model Selector**             | Opens the interactive model selection modal with fuzzy search.                          |
| `Ctrl+P`                 | **Cycle Model Forward**        | Cycles to the next available model in your configured presets.                          |
| `Shift+Ctrl+P` / `Alt+P` | **Cycle Model Backward**       | Cycles to the previous model in your presets.                                           |
| `Shift+Tab`              | **Cycle Thinking Effort**      | Cycles reasoning effort levels (`off` → `minimal` → `low` → `medium` → `high` → `max`). |
| `Ctrl+T`                 | **Toggle Thinking Visibility** | Toggles thinking/reasoning blocks between visible and hidden in the transcript.         |

---

## Transcript & Clipboard Operations

Shortcuts for inspecting tools and copying or pasting content:

| Shortcut | Action                    | Details                                                                                    |
| :------- | :------------------------ | :----------------------------------------------------------------------------------------- |
| `Ctrl+O` | **Toggle Tool Outputs**   | Expands or collapses tool result cards (diffs, bash stdout, file reads) in the transcript. |
| `Ctrl+V` | **Paste Clipboard Image** | Detects an image in the system clipboard and attaches it to the session.                   |
| `Ctrl+X` | **Copy Last Response**    | Copies the assistant's latest message to the system clipboard.                             |

---

## Prompt Editor & Navigation

Standard Readline/Emacs editing and navigation commands supported in the prompt
editor:

| Shortcut                             | Action                                                               |
| :----------------------------------- | :------------------------------------------------------------------- |
| `Left` / `Ctrl+B`                    | Move cursor one character left                                       |
| `Right` / `Ctrl+F`                   | Move cursor one character right                                      |
| `Alt+Left` / `Ctrl+Left` / `Alt+B`   | Move cursor one word backward                                        |
| `Alt+Right` / `Ctrl+Right` / `Alt+F` | Move cursor one word forward                                         |
| `Home` / `Ctrl+A`                    | Move cursor to start of line                                         |
| `End` / `Ctrl+E`                     | Move cursor to end of line                                           |
| `Up` / `Down`                        | Navigate multiline editor lines (or browse history when single-line) |
| `Backspace`                          | Delete character backward                                            |
| `Delete`                             | Delete character forward                                             |
| `Ctrl+W` / `Alt+Backspace`           | Delete word backward                                                 |
| `Alt+D` / `Alt+Delete`               | Delete word forward                                                  |
| `Ctrl+U`                             | Delete from cursor to start of line                                  |
| `Ctrl+K`                             | Delete from cursor to end of line                                    |
| `Ctrl+Y`                             | Paste (yank) most recently deleted text from the kill-ring           |
| `Ctrl+-`                             | Undo last edit                                                       |

---

## Modals & Autocomplete Navigation

When interacting with popups (slash command completion, theme picker, model
selector, permission approvals):

| Shortcut                   | Action            | Details                                                                             |
| :------------------------- | :---------------- | :---------------------------------------------------------------------------------- |
| `Tab`                      | **Accept / Next** | Accepts the highlighted autocomplete suggestion, or moves to next option in modals. |
| `Shift+Tab` / `BackTab`    | **Previous**      | Moves to the previous option in autocomplete or selection modals.                   |
| `Up` / `Down` or `k` / `j` | **Select Item**   | Navigates up and down through modal items.                                          |
| `Enter`                    | **Confirm**       | Confirms the selected modal option or runs the approval action.                     |
| `Escape`                   | **Dismiss**       | Dismisses the modal or autocomplete dropdown without applying changes.              |

---

## Custom Keybindings

You can remap default shortcuts or define custom combinations by creating
`~/.config/rho/keybindings.toml` (or `~/.config/rho/keybindings.json`).

### Configuration Example

```toml
[bindings]
"app.interrupt" = ["escape"]
"app.clear" = ["ctrl+c"]
"app.model.select" = ["ctrl+l", "ctrl+m"]
"app.thinking.cycle" = ["shift+tab"]
"app.tools.expand" = ["ctrl+o"]
"app.message.copy" = ["ctrl+x"]
"app.message.followUp" = ["alt+enter"]
```

### Supported Action Identifiers

| Action ID                  | Default Key             | Description                         |
| :------------------------- | :---------------------- | :---------------------------------- |
| `app.interrupt`            | `escape`                | Cancel active execution / operation |
| `app.clear`                | `ctrl+c`                | Clear current prompt draft          |
| `app.exit`                 | `ctrl+d`                | Exit rho when prompt is empty       |
| `app.suspend`              | `ctrl+z`                | Suspend process to background       |
| `app.editor.external`      | `ctrl+g`                | Open input in external editor       |
| `app.clipboard.pasteImage` | `ctrl+v`                | Paste image from clipboard          |
| `app.model.select`         | `ctrl+l`                | Open model selector modal           |
| `app.model.cycleForward`   | `ctrl+p`                | Cycle model forward                 |
| `app.model.cycleBackward`  | `shift+ctrl+p`, `alt+p` | Cycle model backward                |
| `app.thinking.cycle`       | `shift+tab`             | Cycle thinking effort level         |
| `app.thinking.toggle`      | `ctrl+t`                | Toggle thinking visibility          |
| `app.tools.expand`         | `ctrl+o`                | Toggle tool card expansion          |
| `app.message.copy`         | `ctrl+x`                | Copy last assistant message         |
| `app.message.followUp`     | `alt+enter`             | Queue prompt as follow-up           |
| `app.message.dequeue`      | `alt+up`                | Dequeue last queued message         |
