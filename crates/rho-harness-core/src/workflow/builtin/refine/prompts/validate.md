Validate the candidate findings from a `{{inputs.profile}}` scan. The scout reports are in the context below.

For every candidate:
1. Re-read the cited code yourself. Never accept a candidate on the scout's word.
2. Reject it if it is weak, speculative, style-only, already fixed, or the same issue as an excluded finding.
3. Merge candidates that share a root cause.

Excluded findings (never report these again):
{{inputs.excluded}}

Classify each surviving finding:
- `actionable`: severity high or medium, confidence high or medium, and a concrete fix that preserves intended behavior.
- `low_confidence`: anything else still worth reporting.

Give every finding a stable `fingerprint` of the form `<file>:<short-kebab-case-rule>` with no line numbers, so the same issue is recognized across rounds even when lines move.

Respond with exactly one JSON object in a ```json fenced block, with keys `actionable`, `low_confidence`, and `rejected` (the number of candidates you rejected). Use empty arrays when there is nothing to report.
