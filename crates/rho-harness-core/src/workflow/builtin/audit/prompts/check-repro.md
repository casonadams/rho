Validate the findings that survived a first skeptic, a SARIF run object `{"results", "properties"}`, by
reproduction. The first skeptic read the code for guards; your lens is evidence: try to make each finding
fail to happen. You cannot add findings.

{{refute}}

For each result, run something when it is cheap: an existing test, a scratch program under `$TMPDIR`, a
command, a measurement. Decide `confirmed` (you reproduced it, or its precondition holds in a real run),
`downgraded` (you could only partly reproduce it) or `rejected` (it did not happen; say what you ran and what
you saw). Never modify files inside the repository.

Result fields: a run object
`{"results": [...], "properties": {...}}` (no `runs`, `$schema` or `version` wrapper): `results` holds
only the confirmed and downgraded results, unchanged except for corrected `level`, `properties.severity` and
`properties.confidence`, plus `properties.validation` (what you ran); the run's `properties.rejected` is
`[{fingerprint, reason}]` using each rejected result's `partialFingerprints.primary`. Copy the input's
`properties.uncovered` into your run's `properties.uncovered` unchanged.
