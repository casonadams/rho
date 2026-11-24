Merge the validated findings of every pass. Each entry below is one validator result, a SARIF run object `{"results", "properties"}`, for one
hunt task (a list per pass):

{{check.history}}

Findings that share a root cause collapse into one SARIF `result`: keep the strongest evidence, list every
affected location in `locations`, and keep the highest justified `properties.severity` (with matching
`level`). The same defect found by hunters of different categories is one finding; keep the category where
the fix belongs in `properties.category` and list the others in `properties.alsoCategories`. Distinct defects
in the same file stay separate. Keep `partialFingerprints.primary` stable.

Finish with complete_task. Put a one-line summary in `result` and in `data` a SARIF run object,
`{"results": [...], "properties": {...}}`, with no `runs`, `$schema` or `version` wrapper:
`results` is the merged list (empty when nothing was validated) and `properties.mergedDuplicates` is how many
results were folded into others.
