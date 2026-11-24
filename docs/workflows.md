# Workflows

A workflow is an XML file that describes a multi-agent process. rho runs it
itself: it starts the agents, runs the commands, evaluates the conditions, and
decides when loops stop. Models only do the individual agent steps.

```text
/workflow                         # list workflows and any invalid files
/workflow refine security         # run one; arguments fill inputs in order
/workflow refine security rounds=3
```

Workflows are started only by you, with `/workflow`. They are not exposed to
the model as a tool. While one runs, press `Left` on an empty prompt to watch
its agents, and `Esc` to cancel it.

---

## Where workflows live

Later sources override earlier ones with the same name:

1. built in to rho (`audit`)
2. `~/.agents/workflows/`
3. `<project>/.agents/workflows/`

A workflow is either a single file (`.agents/workflows/hello.xml`) or a
directory:

```text
.agents/workflows/refine/
├── workflow.xml      # the workflow; named after the directory
├── agents/           # agents visible only inside this workflow
│   └── implementor.md
└── prompts/          # long prompts referenced by prompt-file
```

rho ships one built-in workflow, `audit` (below). This repository's
[`.agents/workflows/refine/`](../.agents/workflows/refine/) is a complete
example to copy.

### Built-in: `audit`

`/workflow audit [categories] [paths] [passes] [max_tasks] [scope] [verify]` audits code
across ten categories (correctness, security, performance, reliability,
maintainability, architecture, testing, compatibility, usability, operability).
It maps the repository, runs narrow parallel hunts, has a separate agent try to
disprove each finding, fills gaps, merges duplicates, assesses reachability and
fix risk, and saves `.rho/audit/<run id>/report.md` and `findings.json` (findings
are [SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html)
results). It never edits code.

| Input | Default | Meaning |
|---|---|---|
| `categories` | `all` | Comma-separated category names |
| `paths` | `.` | Git pathspec limiting the audit |
| `passes` | `2` | Hunt passes; later passes cover gaps earlier ones reported |
| `max_tasks` | `40` | Hunt tasks per pass |
| `verify` | `single` | `single` (one skeptic per hunt) or `panel` (two skeptics, one reading the code for guards and one trying to reproduce; a finding survives only if both keep it) |
| `scope` | `repo` | `repo` (every tracked file), `branch:BASE` (changes since `BASE`), or `commits:N` (the last N commits) |

For example, `/workflow audit security,reliability . 1 10 branch:main` audits
what the current branch changed since `main`. Arguments may be quoted
(`paths="src lib"`). To change the built-in, put a workflow named `audit` in
`~/.agents/workflows/` or the project; it replaces it.

---

## Trust

Workflows are trusted. Their commands and their agents' tool calls run without
permission rules, approval prompts, the guard model, or lifecycle hooks. Every
step and every agent tool call is still recorded in the run log
(`.rho/workflows/<name>-<time>-<id>.jsonl`), and `Esc` cancels the whole run.

A workflow never touches your working tree. Each run works in its own git
[worktree](#worktrees), and its changes reach your files only when you or the
main agent merge them.

---

## Format

```xml
<workflow name="refine" description="Scan, fix, verify until clean">
  <input name="profile" choices="review security performance"/>
  <input name="rounds" default="6"/>

  ... steps and blocks ...

  <output name="remaining" value="{{validate.actionable}}"/>
</workflow>
```

| Attribute | Meaning |
|---|---|
| `name` | Defaults to the file or directory name |
| `description` | Shown by `/workflow` |

The `isolation` attribute was removed: every run is isolated in its own
worktree, so a workflow that still sets it fails to load with a message saying
to delete it.

`<input name choices default description/>` declares an input. An input without
a `default` is required; `choices` is a space-separated list of allowed values.
`<output name value/>` declares what the final report shows.

### Steps

| Element | Does | Value it produces |
|---|---|---|
| `<agent use="..." model="..." persona="..." turns="..." resume="1" output="text\|json" context="a b">prompt</agent>` | Runs one agent | Its final text, or the JSON object it returned with `output="json"` |
| `<run cmd="..." output="text\|json"/>` | Runs one command | `{ok, exit, output}`, plus `json` with `output="json"` |
| `<write path=".rho/..." value="..."/>` | Writes a file under the project's `.rho/` | The path written |
| `<call workflow="name" profile="..."/>` | Runs another workflow; other attributes are its inputs | That workflow's outputs |

**`<agent>`**

- `use` names an agent: a file in the workflow's `agents/`, `.agents/agents/`,
  `~/.agents/agents/`, or a built-in role (`scout`, `critic`, `planner`,
  `general`). It may contain `{{references}}` (for example
  `use="hunter-{{task.category}}"`); the name is resolved when the step runs,
  so an unknown agent fails that step instead of the whole workflow up front.
- `model` is a model role from settings: `default`, `smol`, or `slow`
  (default: the persona's or agent's pin in
  [`[subagents.models]`](configuration.md#which-model-runs), else `smol`).
  A literal value is checked when the workflow loads; a `{{reference}}` is
  checked when the step runs.
- `persona` adds a [persona](configuration.md#personas)'s instructions to the
  agent's, keeping the agent's tools. An unknown persona stops the workflow
  before any step runs.
- `turns` limits model calls (default 100).
- The element's text is the prompt, taken verbatim up to `</agent>`, so it can
  contain `<`, `&`, and code. `prompt-file="prompts/x.md"` loads it from a file
  inside the workflow directory instead.
- `context="scan validate"` appends those steps' values to the prompt.
- With `output="json"` the agent passes one JSON object as the `data` argument
  of `complete_task` (an [A2A](https://a2a-protocol.org/latest/specification/)
  data part), so no schema or fence format is needed in the prompt. If it
  writes a fenced JSON block in its final reply instead, that is parsed;
  otherwise one repair call converts its answer, otherwise the step fails.
- `resume="1"` gives an agent that stopped early (turn limit or empty reply)
  that many follow-up rounds in the same saved task, each asking it to finish
  now with what it has. It needs a saved session; without one the step fails
  as before.

**`<run>`**

- `cmd` is split into arguments on spaces first, with `"..."` or `'...'`
  keeping spaces inside one argument. `{{references}}` are filled in afterwards,
  each inside its own argument, so a value never becomes extra arguments or
  shell syntax. There is no shell; write `cmd="sh -c '...'"` when you need one.
  Because `cmd` is split on spaces first, write filters there without spaces:
  `{{tasks|json}}`.
- The command runs in the repository root. A non-zero exit does not stop the
  workflow; it sets `ok` to false so later steps can react.

**`<write>`**

- `path` and `value` are both templates. The path must be relative and inside
  `.rho/` (no `..`); anything else fails the step.
- It writes into the project itself, not the workflow's worktree, so the file
  is still there after the run. Parent directories are created and an existing
  file is replaced atomically.

### Blocks

| Element | Runs its children |
|---|---|
| `<sequence>` | In order (also the body of every other block) |
| `<parallel>` | At the same time; each child is one branch |
| `<each items="..." as="x" parallel="5">` | Once per item, up to `parallel` at a time (default 1); `items` is a `{{reference}}` to a list or a space-separated literal list. A failed item yields `null` and the others continue. Every step in the body runs for an item in order, and items do not wait for each other, so `<each>` with a hunter then a checker in its body is a pipeline: one item's checker starts as soon as its hunter finishes. The item's value is its last step's value |
| `<loop max="6" while="..." until="..." stop-if-unchanged="...">` | Repeatedly, up to `max` passes |
| `<choose on="{{x}}"><case value="a">…</case><otherwise>…</otherwise></choose>` | The first case whose value equals `on` |

A `<loop>` first clears the values of the steps inside it, so every loop starts
fresh; between its passes, a step keeps its last value, which is how one pass
feeds the next. After each pass the loop stops when `until` is truthy, when
`while` is falsy, when `stop-if-unchanged` produces the same value as the
previous pass, or at `max`. Inside the loop, `{{<loop id>.index}}` is the
current pass, starting at 1.

An `<agent>` or `<call>` that fails stops the workflow. A `<run>` whose command
exits non-zero does not; it sets `ok` to false. An agent that stops before a
final answer (turn limit, empty reply, or a failed model call) fails its step
with the reason and its work so far; its summary never becomes the step's
value.

### Conditions

Every step and block accepts `when="{{ref}}"` and `unless="{{ref}}"`. A value
is truthy unless it is empty, `false`, `0`, an empty list or object, or has not
been produced yet. There are no comparisons or arithmetic; decisions are made by
steps (an agent returning `{"approved": true}`, a command's `ok`) and the
workflow branches on their values.

### References

`{{name}}` and `{{name.field.0}}` read:

- inputs, by name
- steps and blocks, by `id`
- the `as` variable inside `<each>`
- `{{<id>.history}}`: every value the step has produced in this run, oldest
  first
- `{{run.id}}`: this run's id (`YYYYMMDD-HHMMSS-xxxxxxxx`), the same one in the
  run log's file name; handy in `<write path>`

Names are checked before the workflow runs, so typos are reported up front. A
step that has not run yet, or was skipped, reads as empty. Strings are inserted
as-is; lists and objects as JSON.

#### Filters

`{{ref | filter | filter:arg}}` transforms a value, left to right. A missing
value is treated as empty. Filter names are checked when the workflow loads.

| Filter | Does |
|---|---|
| `compact` | Drops `null` items from a list (failed `<each>` items) |
| `default:ref` | Uses `{{ref}}` instead when the value is empty, `false`, `0`, or missing |
| `count` | Length of a list or object; `0` when empty or missing |
| `pluck:field` | The `field` of each item in a list, skipping items without it |
| `count_by:field` | `{value: n}` counting items by the value of `field` |
| `json` | Compact one-line JSON text |

For example, `{{findings | compact | count_by:priority}}` gives
`{"P1": 2, "P2": 12}`. `field` may be a dotted path such as
`properties.priority`. A template that is exactly one reference (an `<output>`
value or `each items`) keeps the transformed value's type.

| Step or block | Value |
|---|---|
| `<agent>` | text, or the returned JSON object |
| `<run>` | `{ok, exit, output, json}` |
| `<call>` | the called workflow's outputs |
| `<each>` | a list with the value of each item's last step |
| `<parallel>` | an object keyed by each child's `id` |
| `<loop>` | `{index, passes, stopped}` (`until`, `while`, `unchanged`, or `max`) |
| `<choose>` | the matched case's value, `otherwise`, or empty |

---

## Resuming a run

An agent that finishes without changing files has its result saved next to the run
log, in `.rho/workflows/<name>-<run id>.journal.jsonl`. After a run fails, is
cancelled, or the process dies, `/workflow resume <run id>` (the id is in the log
file name and in `{{run.id}}`) starts the same workflow with the same inputs and
reuses every saved result whose step and prompt are unchanged; only the remaining
steps run. A resume is a new run with its own log and journal, so it can be
resumed again. Editing a prompt, or an upstream result that changed, makes the
affected steps run again. Agents that edited files, and `<run>` and `<write>`
steps, always run again. A workflow named `resume` is shadowed by this command.

---

## Hand-off

Every `<agent>` run is saved to the session file as an
[A2A](https://a2a-protocol.org/latest/specification/) task, like a
[subagent](configuration.md#subagents--custom-agents). The report lists these
under `Agent tasks`, unfinished ones first:

```text
Agent tasks (continue one with the subagent tool's `task_id` to review, fix, or finish its work):
  refine › round 1 › review — task `review-3f9a1c2e` · input-required
```

The report also lists the files each agent changed (through `write`, `edit`,
or changes that appeared while its `bash` commands ran), and a step whose
agent had failed or denied tool calls says so in its detail
(`2 failed tool calls`).

When a run finishes or stops, its report is added in front of your next prompt,
so the main agent sees what happened, with an instruction to verify the work
rather than trust the report. It can review the run's worktree
(`worktree` tool, `diff`), merge or discard it, inspect any listed agent
(`task_id` with `inspect: true`), and continue it with the `subagent` tool's
`task_id`, for example to answer "the reviewer ran out of turns; finish the
review" or "you missed the test, fix it". A continued agent keeps everything it
read and did, but runs under the normal permission rules and hooks: only the
workflow itself is trusted. Agents defined in a workflow's own `agents/`
directory are reloaded from their file. The report is kept in memory until the
next prompt, so it is not handed off if rho exits first; the saved tasks and
the worktree remain.

---

## Worktrees

Every run, and every agent inside it, works in its own git worktree, checked
out under your temp directory (`$TMPDIR/rho-worktrees-<uid>/<repository>/`) on
a branch `rho/wt/<id>`:

1. The run's worktree starts from a snapshot of your working tree, including
   uncommitted and untracked files (your index and files are not touched).
   `<run>` commands run there.
2. Each `<agent>` gets its own worktree from the run's current state. When it
   finishes, rho merges its changes into the run's worktree with a three-way
   merge, so later steps see them. Agents that run at the same time (`<parallel>`,
   `<each>`) are fully isolated; if two of them changed the same lines, the
   merge leaves conflict markers in the run's worktree and the workflow stops,
   handing the conflict to the main agent. An agent that stops before
   finishing is not merged; the report names its worktree.
3. The run's worktree is never merged automatically. The report ends with its
   id, path, and changed files; the main agent (or you, asking it) reviews and
   merges it with the `worktree` tool. A run that changed nothing leaves no
   worktree.

The `worktree` tool's `diff` result begins, and its `merge` result ends, with a
static review of the added code (smells, clean-code violations, duplication; see
[configuration](configuration.md)). Findings never block a merge, and the review
does not run tests or linters.

Top-level ignored entries of your project (`target/`, `node_modules/`, `.env`)
are symlinked into every worktree, so builds reuse caches and dependencies
instead of starting cold; rho adds matching `/<name>` lines to the
repository's `.git/info/exclude` so the links are never committed. Outside a
git repository there are no worktrees and workflows run in the project
directory.

The main agent's session owns the run's worktree and is reminded to merge or
discard it; unmerged worktrees are deleted 24 hours after they were created.
A cancelled run's worktrees are committed, and removed if empty. See
[worktree cleanup](configuration.md#subagents--custom-agents) for the details.

---

## Agents

Agent files say what an agent can do; the workflow says how it runs.

```markdown
---
name: implementor
description: Applies minimal, verified fixes
tools: read, edit, write, fd, rg
---
You fix validated findings...
```

- `tools` is the complete list of rho tools (and MCP tools) the agent may use.
  Omit it to get the read-only tools (`read`, `fd`, `rg`, `web_fetch`,
  `web_search`). `subagent` and `lsp` are never available to agents.
- The model and turn limit come from the caller: the `<agent>` block, or the
  `subagent` tool's `model` and `turns` arguments.
- Every agent's system prompt is grounded like the main session's: the agent
  file's body, usage guidelines for its own tools, `AGENTS.md` instructions,
  the current directory, date, platform, and git status, and, in a workflow,
  which workflow, step, and loop pass it is running.

---

## Example

```xml
<workflow name="refine">
  <input name="profile" choices="review security performance reliability architecture"/>
  <input name="rounds" default="6"/>

  <loop id="round" max="{{rounds}}" while="{{validate.actionable}}"
        stop-if-unchanged="{{validate.actionable}}">
    <each id="scan" items="src crates/rho-engine crates/rho-harness-core" as="slice" parallel="3">
      <agent use="scout" model="smol" turns="40">
        Scan {{slice}} for a {{profile}} review...
      </agent>
    </each>

    <agent id="validate" use="validator" model="smol" output="json" context="scan">
      Re-read every cited line; return {"actionable": [...]}.
    </agent>

    <loop id="attempt" max="3" until="{{review.approved}}" when="{{validate.actionable}}">
      <agent id="implement" use="implementor" model="smol" output="json">
        Fix: {{validate.actionable}}
        Last verification failure: {{verify.output}}
        Last review issues: {{review.issues}}
      </agent>
      <run id="verify" cmd="make all"/>
      <run when="{{verify.ok}}" cmd="git add -A"/>
      <run id="diff" when="{{verify.ok}}" cmd="git diff --cached"/>
      <agent id="review" use="reviewer" model="smol" output="json" when="{{verify.ok}}">
        Review this diff; return {"approved": true|false, "issues": [...]}.
        {{diff.output}}
      </agent>
    </loop>

    <run when="{{review.approved}}" cmd="git commit -m {{implement.commit_subject}}"/>
  </loop>

  <output name="remaining" value="{{validate.actionable}}"/>
</workflow>
```

---

## Watching agents

| Key | Where | Action |
|---|---|---|
| `Left` | empty prompt, or while a workflow runs | Open the **Agents** list |
| type | Agents list | Fuzzy-filter agents |
| `Enter` / `Right` | Agents list | Show the selected agent in the main terminal, in place of the main agent |
| `Delete` | Agents list | Stop the selected running agent; it saves its work and the task can be continued |
| `Left` / `Esc` | Agents list | Close the list |
| type, then `Enter` | agent shown | Message the agent: a running agent reads it before its next model call; for a finished one, the main agent is asked to continue its task (inside a normal, permission-checked turn). Workflow agents cannot be messaged while the workflow runs. |
| `Esc` | agent shown | Stop the agent if it is running (it saves its work first), otherwise return to the main agent |
| `Left` | agent shown, empty prompt | Return to the main agent |

A shown agent looks like the main agent: its prompts, streamed reply, thinking,
and tool cards with live output, and the footer names it and its model
(`◉ audit · claude/claude-haiku-5-5 · turn 3/20`). The main agent keeps working
unseen; its output appears when you return. Slash commands act on the main
session, so they wait until you return. The view follows the agent into the
rounds that continue its task. While subagents run, the footer shows how many
(`⟳ 2 agents (←)`).

When an agent you are not looking at needs a permission answer, the view
switches to it and shows the prompt. With `approval_focus = "indicate"` in
`[subagents]` (see [Configuration](configuration.md#subagents--custom-agents)),
the view stays put, the footer says `⚠ <agent> needs approval`, and the prompt
opens when you show that agent (or return to the main agent, for its own
prompts).
