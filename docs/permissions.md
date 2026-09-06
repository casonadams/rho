# Permissions & Safety

`rho` includes an in-process safety and permission system enabled by default to
protect filesystems and external resources from unintended agent mutations.

---

## Baseline Rules

Commands and tools are classified before execution:

- **Baseline Allowed**: Safe, non-mutating commands run immediately without user
  confirmation. Examples include `git status`, `git diff`, `git log`, `ls`,
  `grep`, `pwd`, and file inspections strictly inside the workspace.
- **Permission Required**: Potentially mutating shell commands (e.g. `rm`,
  `touch`, `curl`, build tools, background daemons) and path operations outside
  the working directory require explicit authorization.

In **headless mode** (`--mode json` or non-interactive runs), permission-gated
operations automatically fail closed with an informative rejection.

---

## Interactive Approval Modal

When an unapproved operation is requested, `rho` displays an interactive modal
with 4 actions:

```text
Tool: bash
Input: cargo test --workspace

[Allow]  [Edit]  [Always]  [Deny]
```

### 1. Allow

Executes the tool call once. The rule is not saved.

### 2. Edit

Opens a multiline editing modal with the command prefilled. Compound commands
linked by `&&`, `;`, or `||` are formatted across separate lines.

- Use **Up/Down/Left/Right** arrow keys to navigate the multiline buffer.
- Edit the text directly.
- Press **Enter** to run the rewritten command, or **Escape** to return to the
  selection bar.

### 3. Always

Persists an allow rule so identical or matching actions run automatically in the
future.

- Selecting **Always** prompts for the match pattern (e.g. `cargo test *` or
  `touch /tmp/*`).
- Press **Enter** to save the rule to `.rho/permission.toml` (project) or
  `~/.config/rho/permission.toml` (global).
- Press **Escape** to cancel and return to the selection bar.

### 4. Deny

Blocks the tool call. Selecting **Deny** opens an input bar to provide an
optional reason or feedback (e.g.
`"Do not remove these files; run git clean -n first"`), which is fed back into
the turn for the model to reconsider its plan.

---

## Configuration & Disabling

Permissions can be disabled per invocation via the command line:

```sh
rho --no-permission
```

Or disabled permanently in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[permission]
enabled = false
```

### Permission Rules (`permission.toml`)

Rules can be manually inspected or authored in `.rho/permission.toml`:

```toml
[tools.bash]
"cargo test *" = "allow"
"npm run build" = "allow"
"rm -rf *" = "deny"

[tools.path]
"/tmp/*" = "allow"
```

### External Permission Plugins

If an external permission plugin (such as `rho-plugin-permission`) is installed
and configured in `config.toml`, `rho`'s built-in permission engine
automatically delegates policy enforcement to the plugin.
