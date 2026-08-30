# Plugins and Extensions

`rho` features an extensible plugin and lifecycle hook system that integrates native Rust binaries, crates.io packages, and manifest-based plugins.

## Activation and discovery

`~/.config/rho/config.toml` is the only activation source for external executable plugins. A declaration may use an absolute path or a path relative to `config.toml`:

```toml
[plugins.container-bash]
path = "/opt/rho/rho-plugin-container-bash"
replaces = ["tool:bash"]

[plugins.review]
path = "plugins/rho-plugin-review"
replaces = []
```

A configured executable is trusted to run with the user's OS permissions. Plugin processes are not OS-sandboxed. Do not configure an executable unless you trust its code and installation path.

`rho plugin list` may also discover matching binaries in Cargo's bin directory, `PATH`, `~/.config/rho/plugins/`, and `.rho/plugins/`. Discovery is informational only: an undeclared binary is reported as unconfigured and is never started or allowed to contribute capabilities.

## Cargo installation and removal

A Cargo package must install a `rho-plugin-<name>` or `rho-<name>` executable. Installation is explicit and requires a local Cargo toolchain:

```bash
rho plugin install rho-plugin-review
rho plugin install rho-plugin-container-bash --replace tool:bash
rho plugin remove review
rho plugin list
rho plugin inspect tool:bash
```

`rho plugin install` runs Cargo, validates protocol-v1 discovery, and atomically writes the executable path, package metadata, and explicitly authorized replacements to `config.toml`. Validation failure leaves configuration unchanged and attempts to uninstall a newly installed package. `rho plugin remove` removes configuration before running Cargo uninstall. Removing a local-path declaration does not delete its executable.

Replacement requires both plugin metadata and the matching `--replace` authorization. Built-ins remain active when a plugin is missing, invalid, conflicting, or lacks replacement authorization.

## Protocol example

[`examples/capability_plugin.rs`](../examples/capability_plugin.rs) is a standalone protocol-v1 subprocess plugin with provider, tool, permission, command, lifecycle, and skill capabilities. It uses no network or credentials.

Build and configure it explicitly for local development:

```bash
cargo build --example capability_plugin
```

```toml
[plugins.fixture]
path = "../../target/debug/examples/capability_plugin"
replaces = []
```

The path is resolved relative to `config.toml`. Rho starts the executable only after it appears in `[plugins]`.

A global tool replacement uses a distinct capability identity and declares the built-in target in both plugin metadata and host configuration:

```toml
[plugins.container-shell]
path = "/opt/rho/rho-plugin-container-shell"
replaces = ["tool:bash"]
```

The replacement is advertised to the model as `bash`, while inspection reports its plugin and capability identities. Rho validates arguments, declared effects, protected paths, network targets, approval, repeated calls, and lifecycle events before dispatch. These checks govern model-requested operations; they do not sandbox the trusted executable itself.

## Skills versus plugins

Skills are declarative, data-only workflow documents; plugins are executable capability providers. Rho resolves skills in precedence order and later roots replace earlier same-name skills:

1. Embedded built-ins (`rho://skills/<name>`)
2. User skills: `<config_dir>/skills/<name>/SKILL.md` (or a flat `<file>.md`)
3. Project skills: `.rho/skills/`, `prompts/skills/`, and `skills/` in the workspace

`/skills` lists every resolved skill with its origin (`built-in`, `user`, `project`), and `/skill <name>` prints the resolved content. Override content is rendered or loaded as data only — skill assets never execute, spawn processes, or touch configuration, unlike plugin executables.

## Credential boundaries

The host owns all credential persistence (`AuthStore`, OAuth token directories). When dispatching a capability operation for a configured plugin, the host supplies only that capability's scoped credential material and never serializes `AuthStore`, token directories, or unrelated secrets across the protocol. Refreshed provider credentials return through a dedicated protocol result for host persistence.

## Failure behavior

A plugin that is missing, invalid, hangs, crashes, or violates the protocol disables only its own capabilities; unrelated built-ins remain active, and the old active capability stays available when an authorized replacement fails. Configured permission policies compose restrictively: deny over approval-required over allow, and a policy failure or invalid result denies operations that require approval. Host-floor denials (schema, workspace, protected locations, network) cannot be overturned by any plugin decision.
