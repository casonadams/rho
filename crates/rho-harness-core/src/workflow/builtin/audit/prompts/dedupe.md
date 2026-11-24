Merge the validated findings of every pass. Each entry below is one validator result, a SARIF run object `{"results", "properties"}`, for one
hunt task (a list per pass):

{{verified.history | json}}

Findings that share a root cause collapse into one SARIF `result`: list every affected location in
`locations`, and keep the highest justified `properties.severity` (with matching `level`). The same defect found by hunters of different categories is one finding; keep the category where
the fix belongs in `properties.category` and list the others in `properties.alsoCategories`. Distinct defects
in the same file stay separate. Keep `partialFingerprints.primary` stable.

Keep the merged output small, because it must fit in one answer: every merged result carries only
`ruleId`, `level`, `message`, `partialFingerprints`, `locations` and `properties` with `category`,
`alsoCategories`, `severity`, `confidence` and `evidence`. Cut `evidence` to the one or two sentences that
prove the defect, with file:line references. Drop `impact`, `recommendation`, `validation` and every other
field; the next step re-reads the code and restores them. Copy nothing else, and never merge findings
whose root cause you have not compared.

Result fields: a SARIF run object,
`{"results": [...], "properties": {...}}` (no `runs`, `$schema` or `version` wrapper):
`results` is the merged list (empty when nothing was validated) and `properties.mergedDuplicates` is how many
results were folded into others.
