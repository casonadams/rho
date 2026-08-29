Ask the user one or more structured questions during execution.

When to use:
- Gather user preferences, constraints, or requirements when instructions are ambiguous or underspecified.
- Get confirmation on implementation trade-offs or architectural directions before making substantial changes.
- Offer choices to the user about what direction to take when multiple valid approaches exist.

Guidelines:
- Each question should have a concise `header` (1-3 words, e.g. "Auth method", "Library", "Approach").
- Provide clear, mutually exclusive `options` (2-4 choices). Each option should have a concise `label` and a clear `description` explaining the trade-off.
- If you recommend a specific option, put it first and append "(Recommended)" to its label.
- For open-ended questions without fixed choices, omit options to let the user answer in freeform text.
- Group all blocking questions into a single `ask_user_question` call instead of asking one by one.
