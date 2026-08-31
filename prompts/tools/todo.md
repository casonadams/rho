Manage a task list for tracking multi-step progress.

Actions:
- create: Create a new task with a subject, optional description, status, and dependencies.
- update: Change status, activeForm, fields, or dependencies of an existing task.
- list: List all tasks, optionally filtered by status (excludes deleted tasks unless includeDeleted is true).
- get: Retrieve full details of a specific task by id.
- delete: Tombstone a task (mark as deleted).
- clear: Reset and delete all tasks.

Usage guidelines:
- Use todo for complex work with 3+ steps, when given a list of tasks, or to capture requirements.
- Exactly one task in_progress at a time. Mark it in_progress with an activeForm spinner before beginning work.
- Mark tasks completed immediately upon finishing.
