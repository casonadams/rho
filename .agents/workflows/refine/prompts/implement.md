Fix these validated `{{profile}}` findings in rho:

{{validate.actionable}}

The workflow runs `make all` after you finish. Its output from the previous attempt, if that attempt failed (empty on the first attempt):

{{verify.output}}

Rules:
- Make the smallest change that removes each root cause. Do not refactor or reformat unrelated code.
- Never make a finding disappear by suppressing it: no lint `allow`/`expect`, no deleted, skipped, or weakened tests, no disabled checks.
- For bug fixes, add a focused regression test in the module's existing test block.
- If a finding is a false positive or cannot be fixed safely, leave the code alone and list it under `wont_fix` with a concrete reason.

Return {"fixed": ["<fingerprint>"], "wont_fix": [{"fingerprint", "reason"}], "files_changed": ["<path>"]}.
