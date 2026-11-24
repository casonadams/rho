Hunt task `{{task.id}}`.

Category: {{task.category}}
Scope: {{task.scope}}
Focus: {{task.focus}}
Hint: {{task.hint}}

Review range: `{{range.output}}`. If it is not `repo`, this audit covers a change: start from
`git diff {{range.output}} -- <scope paths>`, and report findings in the changed code or caused by it.

Repository architecture notes:
{{recon.architecture}}

Apply the `{{task.category}}` rubric from your instructions to the scope. Read outside the scope when you
need to follow data or control flow, but report only findings located inside it. Verify with commands
when that is cheap and turns a suspicion into evidence.

Budget: about 40 tool calls. Once you pass 30, stop exploring and finish with what you have, listing the
rest as uncovered.

The result is a SARIF 2.1.0 run object, `{"results": [...], "properties": {...}}`, not a full SARIF log
(no `runs`, `$schema` or `version` keys).
- `results`: one SARIF `result` per finding. `ruleId` is `<category>/<short-kebab-rule>`; `level` is
  `error` (critical or high), `warning` (medium) or `note` (low); `message.text` is the title;
  `locations[0].physicalLocation` holds `artifactLocation.uri` (repo-relative path) and `region.startLine`;
  `partialFingerprints.primary` is `<file>:<ruleId>` with no line numbers.
- Each result's `properties`: `category`, `severity` (critical|high|medium|low), `confidence`
  (high|medium|low), `evidenceLevel` (reproduced|traced|static), `evidence` (what the code does, with
  file:line citations), `impact`, `recommendation`.
- The run's `properties.uncovered`: `[{scope, category, why}]` for what you touched but did not examine
  closely. Use empty lists when there is nothing to report.
