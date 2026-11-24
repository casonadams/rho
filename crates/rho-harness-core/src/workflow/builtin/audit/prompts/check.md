Validate this hunter result, a SARIF run object `{"results", "properties"}`. Try to disprove every finding; you cannot add findings.

{{found}}

For each result decide `confirmed` (you re-read the code and it holds), `downgraded` (real but less severe
or less certain than claimed) or `rejected` (name the guard, caller, test or rule that defeats it).

Result fields: a run object
`{"results": [...], "properties": {...}}` (no `runs`, `$schema` or `version` wrapper): `results` holds
only the confirmed and downgraded results, unchanged except for corrected `level` and `properties.severity`
and `properties.confidence` where you downgraded, plus `properties.validation` (what you checked); the run's
`properties.rejected` is `[{fingerprint, reason}]` using each rejected result's `partialFingerprints.primary`.
Copy the hunter's `properties.uncovered` into your run's `properties.uncovered` unchanged.
