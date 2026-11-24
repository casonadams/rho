Validate this hunter result, a SARIF run object `{"results", "properties"}`. Try to disprove every finding; you cannot add findings.

{{result}}

For each result decide `confirmed` (you re-read the code and it holds), `downgraded` (real but less severe
or less certain than claimed) or `rejected` (name the guard, caller, test or rule that defeats it).

Finish with complete_task. Put a one-line summary in `result` and a run object
`{"results": [...], "properties": {...}}` in `data`, with no `runs`, `$schema` or `version` wrapper: `results` holds
only the confirmed and downgraded results, unchanged except for corrected `level` and `properties.severity`
and `properties.confidence` where you downgraded, plus `properties.validation` (what you checked); the run's
`properties.rejected` is `[{fingerprint, reason}]` using each rejected result's `partialFingerprints.primary`.
