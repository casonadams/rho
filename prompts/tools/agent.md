Launch a specialized autonomous subagent to perform complex tasks in an isolated context.

Usage:
- Use Agent to delegate research, deep search, planning, or independent multi-step execution.
- Available subagent types: explore, plan, general-purpose, and discovered project/user agents.
- Pass run_in_background: false when the very next action depends on the result.
- Pass run_in_background: true (default) for fire-and-forget or parallel background tasks.
- Use get_subagent_result to check status/results of background jobs.
- Use steer_subagent to send feedback or redirection to running agents.
