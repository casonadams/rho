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

1. `~/.agents/workflows/`
2. `<project>/.agents/workflows/`

A workflow is either a single file (`.agents/workflows/hello.xml`) or a
directory:

```text
.agents/workflows/refine/
├── workflow.xml      # the workflow; named after the directory
├── agents/           # agents visible only inside this workflow
│   └── implementor.md
└── prompts/          # long prompts referenced by prompt-file
```

rho ships no built-in workflows. This repository's
[`.agents/workflows/refine/`](../.agents/workflows/refine/) is a complete
example to copy.

---

## Trust

Workflows are trusted. Their commands and their agents' tool calls run without
permission rules, approval prompts, the guard model, or lifecycle hooks. Every
step and every agent tool call is still recorded in the run log
(`.rho/workflows/<name>-<time>-<id>.jsonl`), and `Esc` cancels the whole run.

---

## Format

```xml
<workflow name="refine" description="Scan, fix, verify until clean" isolation="branch">
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
| `isolation="branch"` | Run on a new branch `rho/<name>-<time>`; refuses to start on a dirty working tree |

`<input name choices default description/>` declares an input. An input without
a `default` is required; `choices` is a space-separated list of allowed values.
`<output name value/>` declares what the final report shows.

### Steps

| Element | Does | Value it produces |
|---|---|---|
| `<agent use="..." model="..." turns="..." output="text\|json" context="a b">prompt</agent>` | Runs one agent | Its final text, or the JSON object it returned with `output="json"` |
| `<run cmd="..." output="text\|json"/>` | Runs one command | `{ok, exit, output}`, plus `json` with `output="json"` |
| `<call workflow="name" profile="..."/>` | Runs another workflow; other attributes are its inputs | That workflow's outputs |

**`<agent>`**

- `use` names an agent: a file in the workflow's `agents/`, `.agents/agents/`,
  `~/.agents/agents/`, or a built-in role (`scout`, `critic`, `planner`,
  `general`).
- `model` is a model role from settings: `default`, `smol`, or `slow`
  (default `smol`).
- `turns` limits model calls (default 100).
- The element's text is the prompt, taken verbatim up to `</agent>`, so it can
  contain `<`, `&`, and code. `prompt-file="prompts/x.md"` loads it from a file
  inside the workflow directory instead.
- `context="scan validate"` appends those steps' values to the prompt.
- With `output="json"` the agent must return one JSON object; if it does not,
  one repair call converts its answer, otherwise the step fails.

**`<run>`**

- `cmd` is split into arguments on spaces first, with `"..."` or `'...'`
  keeping spaces inside one argument. `{{references}}` are filled in afterwards,
  each inside its own argument, so a value never becomes extra arguments or
  shell syntax. There is no shell; write `cmd="sh -c '...'"` when you need one.
- The command runs in the repository root. A non-zero exit does not stop the
  workflow; it sets `ok` to false so later steps can react.

### Blocks

| Element | Runs its children |
|---|---|
| `<sequence>` | In order (also the body of every other block) |
| `<parallel>` | At the same time; each child is one branch |
| `<each items="..." as="x" parallel="5">` | Once per item, up to `parallel` at a time (default 1); `items` is a `{{reference}}` to a list or a space-separated literal list. A failed item yields `null` and the others continue |
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
been produced yet. There is no expression language; decisions are made by steps
(an agent returning `{"approved": true}`, a command's `ok`) and the workflow
branches on their values.

### References

`{{name}}` and `{{name.field.0}}` read:

- inputs, by name
- steps and blocks, by `id`
- the `as` variable inside `<each>`
- `{{<id>.history}}`: every value the step has produced in this run, oldest
  first

Names are checked before the workflow runs, so typos are reported up front. A
step that has not run yet, or was skipped, reads as empty. Strings are inserted
as-is; lists and objects as JSON.

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
or changes that appeared while its `bash` commands ran), other files that
changed in the workspace while agents ran, and a warning when agents that ran
at the same time (for example in `<parallel>` or `<each>`) changed the same
file. A
step whose agent had failed or denied tool calls says so in its detail
(`2 failed tool calls`).

When a run finishes or stops, its report is added in front of your next prompt,
so the main agent sees what happened, with an instruction to verify the work
rather than trust the report. It can inspect any listed agent (`task_id` with
`inspect: true`) and continue it with the `subagent` tool's `task_id`, for example to answer "the
reviewer ran out of turns; finish the review" or "you missed the test, fix
it". A continued agent keeps everything it read and did, but runs under the
normal permission rules and hooks: only the workflow itself is trusted. Agents
defined in a workflow's own `agents/` directory are reloaded from their file.
The report is kept in memory until the next prompt, so it is not handed off if
rho exits first; the saved tasks remain.

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
  Omit it to allow all tools. `subagent` is never available to agents.
- The model and turn limit come from the caller: the `<agent>` block, or the
  `subagent` tool's `model` and `turns` arguments.
- Every agent's system prompt is grounded like the main session's: the agent
  file's body, usage guidelines for its own tools, `AGENTS.md` instructions,
  the current directory, date, platform, and git status, and, in a workflow,
  which workflow, step, and loop pass it is running.

---

## Example

```xml
<workflow name="refine" isolation="branch">
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
| `Enter` / `Right` | Agents list | Watch the selected agent's live transcript |
| `Up`/`Down`, `PgUp`/`PgDn` | agent view | Scroll; `End` (or `G`) follows new output again |
| `Tab` / `Shift+Tab` | agent view | Jump to the next or previous agent |
| `Left` / `Esc` | either | Go back one level |
