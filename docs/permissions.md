# Permissions, Privacy & Safety

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

---

## Guard Model

To streamline development workflows without disabling safety protections, `rho` supports delegating bash command evaluations to a dedicated **Guard Model** (by default recommended as a fast local model: `ollama/qwen2.5-coder:7b`).

When a guard model is configured, benign developer operations (compiling code, executing tests, running linters, inspecting local git history, checking container logs) execute automatically with zero interactive prompts, while hazardous operations (remote git pushes, cloud resource deletion, database mutations, privilege escalation, credential exfiltration) are intercepted with an explicit security explanation.

### Architecture & Multi-Tier Evaluation

Every tool invocation flows through a multi-tiered security pipeline before execution:

1. **Explicit Policy Rules (`permission.toml`)**: Custom `allow`, `deny`, or `ask` patterns defined in `.rho/permission.toml` (project) or `~/.config/rho/permission.toml` (global) take highest precedence. If a command matches an explicit rule, that action is taken immediately without consulting the guard model.
2. **Baseline Safe Classification**: Built-in non-mutating inspection tools (`read`, `fd`, `rg`, `web_search`, `web_fetch`) and non-mutating shell commands (`pwd`, `git status`, `git diff`, `git log`) run immediately within the workspace.
3. **Guard Model Security Evaluation**: For unclassified bash commands, `rho` encapsulates the full command string inside `<command_to_evaluate>` and invokes the configured guard model with a specialized security classification system prompt.
4. **Frictionless Safe Execution**: If the guard model classifies the command as safe (`{"safe": true, "reason": "..."}`), execution proceeds immediately without interrupting your flow.
5. **Targeted Interception**: If classified as unsafe (`{"safe": false, "reason": "..."}`), execution pauses and surfaces the interactive approval modal displaying the guard's specific security rationale.
6. **Fail-Safe Closed Fallback**: If the guard model times out (10 seconds), is offline, encounters an error, or returns unparseable output, execution safely fails closed: interactive mode displays the approval prompt with the failure notice, and headless mode rejects execution. Commands never execute silently on failure.

---

## Guidance on `qwen2.5-coder:7b`

`qwen2.5-coder:7b` (via Ollama or local inference) is strongly recommended for the guard model role:

### Why `qwen2.5-coder:7b`?

- **Deep DevOps & CLI Domain Comprehension**: Pretrained heavily on source code, shell scripts, CLI flags, and DevOps/SRE/Cloud infrastructure tools (git, kubectl, terraform, docker, helm, cloud CLIs, database clients). It accurately discriminates between safe local developer commands (`rm -rf target/`, `git checkout -b fix`, `kubectl get pods`) and hazardous system mutations (`rm -rf /`, `git push --force`, `kubectl delete namespace`).
- **Low Latency & Compact Footprint**: At ~4.7 GB quantized (Q4_K_M), it fits easily into Apple Silicon unified memory (M1/M2/M3/M4 with 8GB+ RAM) or consumer GPUs (RTX 3060/4060+). It evaluates commands in sub-second time, eliminating perceptible CLI lag during rapid autonomous agent turns.
- **Reliable Structured Output**: Operates deterministically at `temperature: 0.0` with non-thinking execution (`thinking_level: None`), consistently returning raw JSON matching `{"safe": boolean, "reason": "..."}` without conversational filler or hallucinated explanations.
- **Air-Gapped Privacy & Zero Data Leakage**: Shell command strings, internal repository paths, CLI arguments, and sensitive parameters remain 100% on-device and are never transmitted to external APIs or third-party cloud relays.
- **Offline Resilience**: Guards remain fully operational during flights, offline travel, or inside strict air-gapped enterprise networks.

### Quick Setup with Ollama

1. **Pull the model locally**:
   ```bash
   ollama pull qwen2.5-coder:7b
   ```

2. **Configure in rho**:
   ```bash
   rho config models.guard ollama/qwen2.5-coder:7b
   ```

   Or edit `~/.config/rho/config.toml` (global) or `.rho/config.toml` (project):
   ```toml
   [models]
   default = "anthropic/claude-3-7-sonnet"
   guard = "ollama/qwen2.5-coder:7b"
   ```

3. **Verify in `/settings`**:
   Type `/settings` in an active REPL session to view or switch the configured **Guard Model**.

To disable the guard model and require manual confirmation for all non-baseline commands:
```bash
rho config models.guard none
```

---

## Security Classification Taxonomy

The guard model evaluates commands against strict operational boundaries:

### Safe Categories (Allowed Automatically)

- **Local Build, Test & Linting**: `cargo`, `go`, `bun`, `npm`, `pnpm`, `yarn`, `pip`, `uv`, `pytest`, `vitest`, `jest`, `make`, `tsc`, `ruff`, `clippy`, and other typecheckers.
- **Local Dependency Resolution**: `cargo check`, `cargo fetch`, `npm install`, `bun install`, `pip install -r requirements.txt` (within project scope).
- **Workspace File Management**: Creating, editing, moving, or cleaning local files inside the project (`touch`, `mkdir`, `cp`, `mv`, `rm -rf target/dist/build/.cache`).
- **Local Version Control**: Non-remote git operations (`git status`, `git diff`, `git log`, `git show`, `git branch`, `git checkout`, `git switch`, `git add`, `git commit`, `git stash`).
- **Read-Only Diagnostics & Inspection**: `ps`, `top`, `htop`, `lsof`, `uname`, `whoami`, `id`, `df`, `du`, `cat`, `grep`, `rg`, `head`, `tail`, `jq`, `yq`.
- **Read-Only Containers & Clusters**: `docker ps`, `docker images`, `docker inspect`, `docker logs`, `kubectl get`, `kubectl describe`, `kubectl logs`.
- **Read-Only Network Diagnostics**: `ping`, `traceroute`, `dig`, `nslookup`, `curl` (GET/HEAD requests without piping to shell).

### Unsafe Categories (Always Pauses for Confirmation)

- **Remote Git Publishing**: ANY `git push` (normal or `--force`), deleting remote branches, or pushing tags.
- **Destructive Git History Loss**: `git reset --hard`, `git clean -fd`, mass discard operations (`git checkout .`, `git restore .`).
- **Destructive Filesystem Deletions**: `rm -rf /`, `rm -rf ~`, `rm -rf *`, recursive deletes outside the project workspace, low-level disk formatting (`mkfs`, `dd if=`, raw writes to `/dev/sd*`), and indiscriminate permission changes (`chmod -R 777`).
- **Infrastructure & Cloud Mutations**: `kubectl apply/create/delete/patch/exec`, `terraform apply/destroy`, `helm install/upgrade/delete`, cloud resource mutations (`aws ... create/delete`, `gcloud ... create/delete`, `az ... create/delete`).
- **System Tampering & Privilege Escalation**: `sudo`, `su`, `doas`, and modifying system files (`/etc`, `/usr`, `/var`, `/System`, `systemctl`, `launchctl`).
- **Secret Exposure & Exfiltration**: Accessing credentials (`~/.ssh`, `~/.aws`, `~/.kube/config`, `.env*` files), piping remote scripts to shells (`curl ... | bash`, `wget ... | sh`), or transmitting sensitive data to external endpoints.
- **Databases & State Mutations**: Mutating SQL queries (`DROP`, `TRUNCATE`, `DELETE`, `ALTER`), database schema migrations, and clearing caches/queues (`redis FLUSHALL/FLUSHDB`, kafka topic deletions).
- **Package & Artifact Publishing**: `cargo publish`, `npm publish`, `twine upload`, `docker push`.
- **Compound Commands**: In compound commands (`&&`, `||`, `;`, `|`), if **any** sub-command or pipeline stage is unsafe, the entire command is classified as unsafe.

---

### Permission Rules (`permission.toml`)

Rules can be manually authored or inspected in `.rho/permission.toml`
(project-scoped) or `~/.config/rho/permission.toml` (global).

#### Basic Rules

```toml
[permission.bash]
"cargo test *" = "allow"
"npm run build" = "allow"
"rm -rf *" = "deny"

[permission.path]
"/tmp/*" = "allow"
```

#### Custom Deny Reasons

You can provide an explicit `reason` for any denied rule using inline tables.
When triggered, the tool execution is skipped and the custom reason is returned
directly to the agent:

```toml
[permission.path]
"*.env*" = { action = "deny", reason = "Do not read or inspect environment secret files" }
"/etc/*" = { action = "deny", reason = "Access to system configuration directories is forbidden" }

[permission.bash]
"cargo publish" = { action = "deny", reason = "Publishing to crates.io is not permitted from the agent" }
"rm -rf *" = { action = "deny", reason = "Destructive command blocked by project safety policy" }
"git push --force*" = { action = "deny", reason = "Force-pushing branches is strictly prohibited" }
```

#### Grouped Syntax

Rules can also be grouped under top-level action tables:

```toml
[allow]
bash = ["cargo test *", "npm test"]

[deny]
bash = ["rm -rf *", "git reset --hard *"]
read = ["*.env*", "/etc/*"]

[ask]
bash = ["curl *", "docker run *"]
```

---

## Privacy & Zero Telemetry

`rho` is built with a zero-telemetry architecture:

- **Zero Data Collection**: `rho` collects **nothing**. There is no telemetry,
  no usage statistics, no crash reporting, and no phone-home pings.
- **Direct Provider Connection**: Prompts, tool calls, and model completions
  travel directly and securely between your machine and your configured model
  provider (Anthropic, OpenAI, Gemini, local Ollama, or custom endpoint). No
  traffic is ever routed through rho servers, proxies, or cloud relays.
- **Local-First Storage**: All authentication tokens (`auth.json`), user
  preferences (`config.toml`), permission rules (`permission.toml`), and session
  transcripts remain strictly on your local filesystem (`~/.config/rho`,
  `~/.local/share/rho`, and project `.rho/`).
- **Auditable Dependencies**: The repository contains zero analytics, tracking,
  or telemetry crates. Every outbound network call is initiated strictly by your
  configured providers or explicit tool invocations (e.g. `web_search` or
  `web_fetch`).
