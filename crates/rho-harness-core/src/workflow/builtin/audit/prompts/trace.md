Assess this confirmed finding, a SARIF result.

{{finding}}

Repository architecture notes:
{{recon.architecture}}

Answer, from the code:
- `reachability`: the shortest real path from an entry point to the flaw, who controls the inputs, and
  how likely the trigger is in practice (`likely`, `unlikely`, `unreachable`). For performance findings
  state whether the path is hot and, if you can, measure it. For maintainability and testing findings,
  state how often the code changes and how costly the weakness is for the people changing it.
- `mitigations`: existing guards, limits, or tests that reduce the impact.
- `fix`: the approach, what else depends on the code being changed, what a fix must preserve, the
  regression risk (`low`, `medium`, `high`), and the tests that must exist.
- `priority`: P0 (fix now: critical and likely), P1 (high, or critical but unlikely), P2 (medium, or
  high but unlikely), P3 (low, or anything unreachable), with the reason in `priorityReason`.

Finish with complete_task. Put a one-line summary in `result` and in `data` the same SARIF result with
those added under `properties` as `reachability` ({path, likelihood}), `mitigations`, `fix`
({approach, mustPreserve, regressionRisk, tests}), `priority` and `priorityReason`. Keep everything else
unchanged.
