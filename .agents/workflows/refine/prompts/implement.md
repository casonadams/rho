Fix these validated `{{profile}}` findings in rho:

{{validate.actionable}}

After you finish, the workflow runs `make all` and then a reviewer reads your staged diff. Feedback from the previous attempt, if it failed (both are empty on the first attempt):

`make all` output:
{{verify.output}}

Reviewer issues:
{{review.issues}}

Rules:
- Make the smallest change that removes each root cause. Do not refactor or reformat unrelated code.
- For `performance` and `architecture` findings, preserve observable behavior exactly, including first-call versus repeated-call behavior, state supplied at construction, and behavior after an earlier error.
- Never weaken a security or safety check, even as a side effect of an optimization.
- Never make a finding disappear by suppressing it: no lint `allow`/`expect`, no deleted, skipped, or weakened tests, no changed assertions in existing tests, no disabled checks.
- Extend or reuse an existing function instead of adding a near copy of it.
- For bug fixes, add a focused regression test in the module's existing test block. Tests must not depend on real time: no sleeping in Rust, spawned commands, or scripts, and no wall-clock assertions.
- If a finding is a false positive or cannot be fixed safely, leave the code alone and list it under `wont_fix` with a concrete reason.

Write the commit message for this change:
- `commit_subject`: a Conventional Commit subject in short imperative mood describing what changed, e.g. `perf(session): defer active-path rebuild during replay`. Use `perf` for performance fixes, `fix` for security, reliability, and review bugs, and `refactor` for architecture changes. The scope names the area changed, never the profile.
- `commit_body`: one line per fixed finding, starting with its fingerprint.

Result fields: {"fixed": ["<fingerprint>"], "wont_fix": [{"fingerprint", "reason"}], "files_changed": ["<path>"], "behavior_changes": ["<observable change, or empty>"], "preserved_invariants": ["<behavior that must stay the same and how the change keeps it>"], "commit_subject": "<subject>", "commit_body": "<body>"}.
