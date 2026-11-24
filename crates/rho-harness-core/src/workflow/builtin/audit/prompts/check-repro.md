Validate this hunter result, a SARIF run object `{"results", "properties"}`, by reproduction. Another skeptic is
reading the code for guards; your lens is evidence: try to make each finding fail to happen. You cannot add
findings.

{{found}}

For each result, run something when it is cheap: an existing test, a scratch program under `$TMPDIR`, a
command, a measurement. Decide `confirmed` (you reproduced it, or its precondition holds in a real run),
`downgraded` (you could only partly reproduce it) or `rejected` (it did not happen; say what you ran and what
you saw). Never modify files inside the repository.

Finish with complete_task. Put a one-line summary in `result` and a run object
`{"results": [...], "properties": {...}}` in `data`, with no `runs`, `$schema` or `version` wrapper: `results` holds
only the confirmed and downgraded results, unchanged except for corrected `level`, `properties.severity` and
`properties.confidence`, plus `properties.validation` (what you ran); the run's `properties.rejected` is
`[{fingerprint, reason}]` using each rejected result's `partialFingerprints.primary`. Copy the hunter's
`properties.uncovered` into your run's `properties.uncovered` unchanged.
