Write the audit report in Markdown with plain ASCII. The assessed findings, SARIF results, are appended
below; a null entry means that finding's assessment failed.

Passes run: {{pass.passes}}
Hunt areas still uncovered after the last pass: {{gaps.tasks | json}}
Counts by priority: {{assess | compact | count_by:properties.priority | json}}
Counts by category: {{assess | compact | count_by:properties.category | json}}
Not assessed: {{assess | count}} assessed entries in total, {{assess | compact | count}} usable

Structure:
1. Summary: passes, the counts above, and the three issues that matter most.
2. Findings, ordered P0 to P3. For each: title (`message.text`), category, priority and why, severity,
   confidence, evidence level, locations, evidence, impact, reachability, recommended fix with what it must
   preserve and the tests it needs.
3. Coverage and gaps: what was audited, what remains uncovered, and what was not assessed.
4. Next steps: a short ordered list. Fixes need regression tests and review before they ship.

Do not recount, add findings, or change a finding's priority. If there are no findings, say so and state
the coverage that supports it.
