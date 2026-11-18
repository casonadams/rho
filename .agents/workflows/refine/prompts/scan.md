You are scanning `{{slice}}` in the rho repository for a `{{profile}}` review. This is analysis only: do not edit files.

Read outside your slice whenever you need to follow data or control flow, but only report findings located inside it. Read AGENTS.md first; findings that ask for something it forbids are not findings.

Guidance for the `{{profile}}` profile:

- review: prioritize correctness and regression risks, then test gaps, maintainability problems, and safe simplifications. Recommend DRY only for duplicated knowledge that must change together. Reject style-only rewrites and abstractions that add cognitive load.
- security: trace untrusted input (model output, tool arguments, repository files, MCP servers, HTTP responses) to sinks (commands, file paths outside the workspace, permission decisions, credential storage, logs). Check that secrets never reach logs, transcripts, or error messages. Reject findings that need an attacker position the code does not expose.
- performance: find super-linear work on growing inputs (transcripts, sessions, tool output), blocking I/O on async paths, and redundant redraws or clones in the TUI loop. Without a benchmark or profile, label every finding a structural hypothesis. Reject micro-optimizations without evidence the path is hot.
- reliability: trace failure paths across provider calls, subprocesses, file writes, cancellation, and terminal state. Check timeouts, retries, cleanup on drop, raw-mode restoration, and partial failure. Reject hypothetical concerns not supported by concrete runtime behavior.
- architecture: check the dependency direction `rho -> rho-engine -> rho-harness-core`, that domain rules stay in `rho-harness-core` free of I/O, and that adapters stay thin. Reject layering concerns not supported by concrete coupling or duplication.

Earlier rounds' fix reports, including findings marked won't-fix (do not report those again):
{{implement.history}}

For each candidate finding give file:line evidence, what is wrong, its impact, your confidence (high, medium, or low), and a concrete recommendation. If evidence is insufficient, say so instead of overstating it. End with the areas of your slice you did not read.
