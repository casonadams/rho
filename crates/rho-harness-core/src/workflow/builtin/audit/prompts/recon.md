Audit this repository. Selected categories, comma-separated: `{{categories}}` (`all` means every category below).
Categories: correctness, security, performance, reliability, maintainability, architecture, testing,
compatibility, usability, operability.

Scope: `{{range.output}}`. `repo` means every tracked file under `{{paths}}`. Anything else is a git range: only
the files below changed in it, so audit that change, not the whole repository. Each task's `hint` must then
tell the hunter the range, so it starts from `git diff <range> -- <scope paths>` and reports problems in the
changed code or that the change causes in code that depends on it.

Files in scope (the list may be truncated, use fd or git for the rest):
{{files.output}}

Produce:

1. `architecture`: a compact document for the hunters covering languages and build, test and lint
   commands, the major subsystems and their responsibilities, dependency direction, entry points, trust
   boundaries (where untrusted input enters), and the repository's own quality rules from AGENTS.md or
   similar.

2. `tasks`: at most {{max_tasks}} tasks, each one category paired with a narrow scope. A good task is
   "security: command injection and path traversal in the tool-execution module, trust boundary is model
   output", not "find security problems". Rules:
   - Scope is one subsystem, module, or cross-cutting flow, small enough for one agent to read closely.
   - Pick category and scope pairs by risk: skip pairs that cannot apply (no performance task for a docs
     directory). Every selected category must get at least two tasks where it applies, and no category may take
     more than a third of the queue; allocate the budget across categories before choosing scopes.
   - Include only selected categories. `category` must be exactly one lowercase name from the list above:
     it selects the agent `hunter-<category>`, and any other value fails that task.
   - `hint` carries what the hunter needs and cannot guess: the trust boundary above the scope, related
     files outside it, and relevant architecture notes.

Result fields:
`{"architecture": "<text>", "tasks": [{"id": "r-01", "category": "<one category>", "scope": ["<path>"], "focus": "<specific question>", "hint": "<context>"}]}`.
