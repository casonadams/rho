Two skeptics verified the same hunter result with different lenses. Combine their verdicts: a finding survives
only if both kept it. You cannot add findings or re-judge them yourself.

Hunter result:
{{found}}

Skeptic that read the code for guards:
{{refute}}

Skeptic that tried to reproduce:
{{repro}}

Match findings by `partialFingerprints.primary`. Keep a finding only when it appears in both skeptics' `results`;
use the lower `properties.severity` and `properties.confidence` of the two (and the matching `level`), and join
their `properties.validation` texts. A finding either skeptic rejected goes to `properties.rejected` with both
reasons.

Finish with complete_task. Put a one-line summary in `result` and a run object
`{"results": [...], "properties": {...}}` in `data`, with no `runs`, `$schema` or `version` wrapper. Copy the
hunter's `properties.uncovered` into your run's `properties.uncovered` unchanged.
