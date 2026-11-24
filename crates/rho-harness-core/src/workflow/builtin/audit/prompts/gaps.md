Plan the next hunt pass. The hunters of this pass reported areas they touched but did not cover closely
(each SARIF run's `properties.uncovered`); their results are appended below.

Tasks already run, never repeat them:
- first pass: {{recon.tasks | json}}
- follow-up passes: {{gaps.history | json}}

Create follow-up tasks only for uncovered areas that are worth the cost: skip anything already covered by a
task above, trivial, or outside the repository's own code. Merge overlapping areas into one task. At most
{{max_tasks}} tasks. Each task is one category (exactly one of the audit categories) paired with a narrow
scope, as in the first pass. Give every task an id starting with `p{{pass.index}}-`.

Result fields:
`{"tasks": [{"id": "p{{pass.index}}-01", "category": "...", "scope": ["<path>"], "focus": "<specific question>", "hint": "<context>"}]}`.
An empty `tasks` list ends the audit's hunting.
