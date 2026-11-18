Review this staged change, made to fix validated `{{profile}}` findings in rho.

Findings it was meant to fix:
{{validate.actionable}}

The implementor's report, including the behavior it claims to change and preserve:
{{implement}}

Staged diff:
{{diff.output}}

Reject the change if any of these hold:
- It changes observable behavior beyond what the findings require. For `performance` and `architecture` fixes, behavior must be identical. Check the first call versus repeated calls, state supplied at construction or startup, and behavior after an earlier error.
- It weakens a security or safety check: credential guards, redaction, permissions, validation, or error propagation. A check that used to fire on every call must still fire on every call.
- It deletes, skips, or rewrites the assertions of an existing test, or adds a test whose assertions encode weakened behavior.
- It adds a function that duplicates an existing one instead of extending or reusing it.
- A test depends on real time: sleeping in Rust, in a spawned command, or in a shell script, or asserting on elapsed wall-clock time.
- It adds comments that restate code, or it breaks another AGENTS.md rule.
- `preserved_invariants` or `behavior_changes` in the report do not match what the diff actually does.

Each issue must cite file:line and say what to change. Do not reject for style preferences.

Return {"approved": true|false, "issues": [{"file", "line", "problem", "fix"}]}. Approve only when `issues` is empty.
