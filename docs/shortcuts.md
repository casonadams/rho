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

| Shortcut                 | Action                         | Details                                                                                           |
| :----------------------- | :----------------------------- | :------------------------------------------------------------------------------------------------ |
| `Ctrl+L`                 | **Model Selector**             | Opens the interactive model selection modal with fuzzy search.                                    |
| `Ctrl+P`                 | **Cycle Model Forward**        | Cycles to the next available model in your configured presets.                                    |
| `Shift+Ctrl+P` / `Alt+P` | **Cycle Model Backward**       | Cycles to the previous model in your presets.                                                     |
| `Shift+Tab`              | **Cycle Thinking Effort**      | Cycles reasoning effort levels (`off` → `minimal` → `low` → `medium` → `high` → `xhigh` → `max`). |
| `Ctrl+T`                 | **Toggle Thinking Visibility** | Toggles thinking/reasoning blocks between visible and hidden in the transcript.                   |

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

When interacting with popups (slash command completion, model selector,
permission approvals):

| Shortcut                   | Action            | Details                                                                             |
| :------------------------- | :---------------- | :---------------------------------------------------------------------------------- |
| `Tab`                      | **Accept / Next** | Accepts the highlighted autocomplete suggestion, or moves to next option in modals. |
| `Shift+Tab` / `BackTab`    | **Previous**      | Moves to the previous option in autocomplete or selection modals.                   |
| `Up` / `Down` or `k` / `j` | **Select Item**   | Navigates up and down through modal items.                                          |
| `Enter`                    | **Confirm**       | Confirms the selected modal option or runs the approval action.                     |
| `Escape`                   | **Dismiss / Back**| Dismisses the modal or autocomplete dropdown, or navigates back one level in hierarchical submenus (`/settings`, `Models`, `Tools & Permissions`). |

---

## Interactive Slash Commands

Type `/` in the prompt editor to trigger auto-complete across all built-in
commands:

| Command     | Action                           | Details                                                                       |
| :---------- | :------------------------------- | :---------------------------------------------------------------------------- |
| `/settings` | **Runtime Settings**             | Interactive settings modal preserving stack hierarchy: switch role models (default, smol, slow, guard), configure thinking, styling, and tools in-place. |
| `/model`    | **Model Switcher**               | Interactive fuzzy model selector across all configured providers.             |
| `/thinking` | **Thinking Level**               | Set reasoning effort (`off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). |
| `/resume`   | **Resume Session**               | Interactive session selector to resume historical conversations.              |
| `/compact`  | **Manual Compaction**            | Summarizes earlier turns to reclaim context window capacity.                  |
| `/tree`     | **Conversation Tree**            | Visualize turn DAG, switch branches, and label checkpoints.                   |
| `/rewind`   | **Rewind Session**               | Rewind conversation history to an earlier checkpoint turn.                    |
| `/fork`     | **Fork Session**                 | Forks session from a specific turn into an isolated conversation.             |
| `/clone`    | **Clone Session**                | Duplicates active branch into a new session file.                             |
| `/name`     | **Name Session**                 | Assigns a human-readable display name to the session.                         |
| `/clear`    | **New Session**                  | Starts a fresh session while preserving past transcripts.                     |
| `/login`    | **Provider Auth**                | Configure OAuth or verify API keys for 13+ providers.                         |
| `/mcp`      | **MCP Servers**                  | Inspect configured MCP servers and capabilities.                              |
| `/export`   | **Export Transcript**            | Generates an HTML or Markdown transcript file.                                |
| `/exit`     | **Exit rho**                     | Shuts down the interactive REPL.                                              |
