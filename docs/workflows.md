# Workflows

A workflow is a TOML file that describes a multi-agent process step by step.
The engine runs the steps itself: it fans out agents, enforces tool
restrictions, runs verification commands, and decides when a loop stops. The
model only does the individual steps. Compare this with a prompt that asks the
model to orchestrate, which it can skip or cut short.

Run a workflow with its slash command, for example:

```text
/refine security
/refine review 3 verify="cargo test"
/workflows          # list workflows, their inputs, and any invalid files
```

The slash command asks the main agent to call the `workflow` tool once with
the bound inputs. While it runs, press `Left` on an empty prompt to watch its
agents (see [Watching agents](#watching-agents)).

---

## Built-in: `refine`

`/refine <profile> [max_rounds] [verify]` is an unattended scan → fix → verify
loop:

1. **scopes**: splits the repository into up to 3 (or 5 for large repos)
   disjoint slices.
2. **scan**: one read-only `scout` per slice, in parallel, guided by the
   profile (`review`, `security`, `performance`, `reliability`,
   `architecture`).
3. **validate**: a read-only `validator` re-reads every cited line, rejects
   weak or style-only items, merges duplicates, and returns JSON findings with
   stable fingerprints. Only high/medium severity and confidence findings are
   actionable.
4. **implement**: an `implementor` fixes the actionable findings or marks them
   `wont_fix` with a reason. `verify` (default `make all`) must pass. On
   failure the implementor gets the failure output and retries up to twice. If
   it still fails, the round's changes are stashed (`git stash list`).
5. **checkpoint**: commits the round.

Rounds repeat until a scan finds nothing actionable (`clean`), the same
findings come back (`no progress`), or `max_rounds` (default 6) is reached.
Findings marked `wont_fix` are carried into later rounds so they are not
reported again.

`refine` runs on a new branch `rho/refine-<timestamp>` and refuses to start on
a dirty working tree, so every change can be reviewed or reverted per round.
It never pushes or merges.

Override it by creating `.agents/workflows/refine/` in your project or
`~/.agents/workflows/refine/` for your user.

---

## Where workflows live

Later sources override earlier ones with the same name:

1. Built-in workflows shipped with rho
2. `~/.agents/workflows/`
3. `<project>/.agents/workflows/`

A workflow is either a single file (`.agents/workflows/hello.toml`) or a
bundle directory:

```text
.agents/workflows/refine/
├── workflow.toml       # entry point; the workflow is named after the directory
├── round.toml          # sub-workflow, called as `workflow = "round"`
├── agents/             # agents visible only inside this bundle
│   └── validator.md
└── prompts/            # files referenced by `prompt_file`
    └── scan.md
```

Agent names resolve in this order: the bundle's `agents/`, then
`.agents/agents/` and `~/.agents/agents/`, then the built-in roles (`scout`,
`critic`, `planner`, `general`). Bundle agents use the same markdown format as
[custom agents](configuration.md). An agent's `tools` list is a hard limit: a
step cannot widen it.

Every workflow is validated before anything runs. Unknown step references,
misplaced options, calls to missing workflows, and mismatched `with`/`carry`
names are reported by `/workflows` and by the `workflow` tool.

---

## Format reference

```toml
name = "refine"                 # optional; defaults to the file or directory name
description = "Scan, fix, verify until clean"
isolation = "branch"            # optional: run on a fresh git branch (needs a clean tree)

[[input]]
name = "profile"
choices = ["review", "security"]   # optional
# no default: required

[[input]]
name = "max_rounds"
default = 6

[[step]]
id = "scan"                     # letters, digits, '-', '_'
agent = "scout"
over = "scopes.items"           # optional: run once per list item, in parallel
concurrency = 5                 # default 4
prompt = "Scan {{item.paths}} for {{inputs.profile}}"

[outputs]
findings = "validate.actionable"                        # one reference
excluded = ["inputs.excluded", "implement.wont_fix"]    # lists are concatenated
```

Steps run in file order. Each step is exactly one of:

| Kind | Fields | Result |
|---|---|---|
| `agent = "<name>"` | `prompt` or `prompt_file`, `max_turns`, `model` | The agent's final text, or a JSON object with `output = "json"` |
| `run = "<command>"` | `timeout_sec` (default 1800) | Trimmed stdout, or parsed JSON with `output = "json"`; a non-zero exit fails the step |
| `builtin = "repo_scopes"` | `with = { max_scopes = 3 }` | `{ items: [{ paths, files }], total_files, reviewable_files, truncated }` |
| `builtin = "git_commit"` | `with = { message = "..." }` | `{ committed, sha }`; nothing to commit is not an error |
| `workflow = "<name>"` | `with`, `repeat`, `carry` | The called workflow's outputs |

Options available on any step:

- `context = ["scan", ...]`: appends earlier steps' results to an agent prompt.
- `when = "validate.actionable"`: skips the step when the value is empty,
  false, zero, or missing.
- `expected_output = "..."`: describes the result shape to the agent.
- `output = "json"`: the agent must return one JSON object. If it doesn't,
  one repair call converts its answer; otherwise the step fails.
- `check = "<command>"`, `retries = 2`, `on_fail = "stop" | "stash" | "continue"`
  (agent steps only): runs a verification command after the agent finishes.
  On failure the agent is re-run with the failure output. When retries run
  out, `stop` ends the workflow, `stash` stashes the changes and continues,
  and `continue` just continues.

Repeating a sub-workflow:

```toml
[[step]]
id = "rounds"
workflow = "round"
with = { profile = "{{inputs.profile}}" }
repeat = { until_empty = "actionable", max = "{{inputs.max_rounds}}", stop_on_no_progress = true }
carry = ["excluded"]            # output `excluded` of round N becomes input `excluded` of round N+1
```

The loop stops when the `until_empty` output is empty (`clean`), when its
items' `fingerprint` fields match the previous round (`no progress`), or at
`max`. The step result is the last round's outputs plus `rounds` and
`stop_reason`.

### Templates

`{{ path }}` placeholders read from `inputs.<name>`, `item` (inside `over`
steps), `round` (the current repeat round, starting at 1), and earlier step
ids, with `.` for fields and list indexes (`{{scan.0}}`). Strings are inserted
as-is; lists and objects as pretty JSON. In a `workflow` step's `with`, a value
that is exactly one placeholder passes the value through unchanged, so lists
stay lists.

---

## Safety

- `run`, `check`, and the git builtins go through the same permission rules
  and lifecycle hooks as the model's `bash` tool.
- Agent tool lists are enforced, so read-only agents cannot edit files.
- Esc cancels the whole workflow, including running agents and commands.
- Each run writes a JSONL event log to `.rho/workflows/<name>-<time>-<id>.jsonl`,
  and the final report links to it.

---

## Watching agents

Every subagent, whether it comes from a workflow or a plain `subagent` call,
is tracked for the session.

| Key | Where | Action |
|---|---|---|
| `Left` | empty prompt | Open the **Agents** list (running agents first, with group, turn, and elapsed time) |
| type | Agents list | Fuzzy-filter agents |
| `Enter` / `Right` | Agents list | Watch the selected agent's live transcript |
| `Up`/`Down`, `PgUp`/`PgDn` | agent view | Scroll; `End` (or `G`) follows new output again |
| `Tab` / `Shift+Tab` | agent view | Jump to the next or previous agent in the list |
| `Left` / `Esc` | either | Go back one level (agent view → list → prompt) |

`Esc` in these views only closes the view; it never cancels the running turn.
