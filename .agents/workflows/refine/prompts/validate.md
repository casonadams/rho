Validate the candidate findings from a `{{profile}}` scan of rho. The scout reports are in the context below.

For every candidate:
1. Re-read the cited code yourself. Never accept a candidate on the scout's word.
2. Reject it if it is weak, speculative, style-only, already fixed, conflicts with AGENTS.md, or was marked won't-fix in an earlier round.
3. Merge candidates that share a root cause.

Earlier rounds' fix reports:
{{implement.history}}

Classify each surviving finding:
- `actionable`: severity high or medium, confidence high or medium, and a concrete fix that preserves intended behavior and passes `make all`.
- `low_confidence`: anything else still worth reporting.

Give every finding a stable `fingerprint` of the form `<file>:<short-kebab-case-rule>` with no line numbers, so the same issue is recognized across rounds.

Return {"actionable": [{"fingerprint", "severity", "confidence", "file", "line", "title", "evidence", "recommendation"}], "low_confidence": [...], "rejected": <count>}. Use empty arrays when there is nothing to report.
