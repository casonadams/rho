Summarize the audit report below for someone reading it in a terminal: at most 12 lines of plain ASCII, no
Markdown headings or tables.

Scope: {{range.output}}
Findings: {{assess | compact | count}} {{assess | compact | count_by:properties.priority | json}}
Full report: {{saved_report}}

Say how many findings there were and their priorities, then list up to the three most important as
`P<n> file:line - title`, then name the report file. If there are no findings, say so and what was covered.
Do not add anything the report does not say.
