Fix these validated `{{inputs.profile}}` findings (JSON):

{{validate.actionable}}

Rules:
- Make the smallest change that removes each root cause. Do not refactor or reformat unrelated code.
- Never make a finding disappear by suppressing it: no lint `allow` attributes or suppressions, no deleted, skipped, or weakened tests, no disabled checks.
- Add or update a focused test when you change observable behavior.
- If a finding is a false positive or cannot be fixed safely, leave the code alone and list it under `wont_fix` with a concrete reason.
- Do not commit, push, or use the network. The workflow commits after verification passes.
- Run `{{inputs.verify}}` before you finish and make sure it passes.

Respond with exactly one JSON object in a ```json fenced block with keys `fixed` (fingerprints you fixed), `wont_fix` (objects with `fingerprint` and `reason`), and `files_changed` (paths you modified).
