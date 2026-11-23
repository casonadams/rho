//! Language server definitions: the built-in table, `lsp.json` overrides, and which servers
//! handle a file.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(skip)]
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub extension_to_language: BTreeMap<String, String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
    /// Linters add diagnostics next to the main server for a file but never answer navigation.
    #[serde(default)]
    pub linter: bool,
}

struct DefaultServer {
    name: &'static str,
    command: &'static str,
    args: &'static [&'static str],
    extensions: &'static [&'static str],
    root_markers: &'static [&'static str],
}

const fn default_server(
    name: &'static str,
    command: &'static str,
    args: &'static [&'static str],
    extensions: &'static [&'static str],
    root_markers: &'static [&'static str],
) -> DefaultServer {
    DefaultServer {
        name,
        command,
        args,
        extensions,
        root_markers,
    }
}

const DEFAULT_SERVERS: &[DefaultServer] = &[
    default_server(
        "rust-analyzer",
        "rust-analyzer",
        &[],
        &["rs"],
        &["Cargo.toml", "rust-project.json"],
    ),
    default_server("gopls", "gopls", &[], &["go"], &["go.work", "go.mod"]),
    default_server(
        "typescript-language-server",
        "typescript-language-server",
        &["--stdio"],
        &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
        &["tsconfig.json", "jsconfig.json", "package.json"],
    ),
    default_server(
        "pyright",
        "pyright-langserver",
        &["--stdio"],
        &["py", "pyi"],
        &[
            "pyproject.toml",
            "pyrightconfig.json",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ],
    ),
    default_server(
        "basedpyright",
        "basedpyright-langserver",
        &["--stdio"],
        &["py", "pyi"],
        &[
            "pyproject.toml",
            "pyrightconfig.json",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ],
    ),
    default_server(
        "clangd",
        "clangd",
        &[],
        &["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx", "m", "mm"],
        &[
            "compile_commands.json",
            "compile_flags.txt",
            ".clangd",
            "CMakeLists.txt",
        ],
    ),
    default_server("zls", "zls", &[], &["zig"], &["build.zig"]),
    default_server(
        "lua-language-server",
        "lua-language-server",
        &[],
        &["lua"],
        &[".luarc.json", ".luarc.jsonc"],
    ),
    default_server(
        "jdtls",
        "jdtls",
        &[],
        &["java"],
        &["pom.xml", "build.gradle", "build.gradle.kts"],
    ),
    default_server(
        "kotlin-language-server",
        "kotlin-language-server",
        &[],
        &["kt", "kts"],
        &["settings.gradle", "settings.gradle.kts", "build.gradle.kts"],
    ),
    default_server("ruby-lsp", "ruby-lsp", &[], &["rb"], &["Gemfile"]),
    default_server("sourcekit-lsp", "sourcekit-lsp", &[], &["swift"], &["Package.swift"]),
    default_server(
        "haskell-language-server",
        "haskell-language-server-wrapper",
        &["--lsp"],
        &["hs"],
        &["stack.yaml", "cabal.project", "*.cabal", "hie.yaml"],
    ),
    default_server("ocamllsp", "ocamllsp", &[], &["ml", "mli"], &["dune-project", "*.opam"]),
    default_server("csharp-ls", "csharp-ls", &[], &["cs"], &["*.sln", "*.csproj"]),
    default_server("elixir-ls", "elixir-ls", &[], &["ex", "exs"], &["mix.exs"]),
    default_server(
        "intelephense",
        "intelephense",
        &["--stdio"],
        &["php"],
        &["composer.json"],
    ),
    default_server(
        "bash-language-server",
        "bash-language-server",
        &["start"],
        &["sh", "bash"],
        &[".git"],
    ),
    default_server(
        "terraform-ls",
        "terraform-ls",
        &["serve"],
        &["tf", "tfvars"],
        &[".terraform", ".git"],
    ),
    default_server("nil", "nil", &[], &["nix"], &["flake.nix"]),
    default_server("taplo", "taplo", &["lsp", "stdio"], &["toml"], &[".git"]),
];

const LANGUAGE_IDS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("go", "go"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "typescriptreact"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascriptreact"),
    ("py", "python"),
    ("pyi", "python"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
    ("hxx", "cpp"),
    ("m", "objective-c"),
    ("mm", "objective-cpp"),
    ("zig", "zig"),
    ("lua", "lua"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("rb", "ruby"),
    ("swift", "swift"),
    ("hs", "haskell"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("cs", "csharp"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("php", "php"),
    ("sh", "shellscript"),
    ("bash", "shellscript"),
    ("tf", "terraform"),
    ("tfvars", "terraform-vars"),
    ("nix", "nix"),
    ("toml", "toml"),
];

fn default_server_values() -> Map<String, Value> {
    DEFAULT_SERVERS
        .iter()
        .map(|server| {
            let value = json!({
                "command": server.command,
                "args": server.args,
                "extensions": server.extensions,
                "rootMarkers": server.root_markers,
            });
            (server.name.to_string(), value)
        })
        .collect()
}

/// Overlays `layer` onto `base` server by server, field by field.
fn merge_layer(base: &mut Map<String, Value>, layer: Map<String, Value>) {
    for (name, overrides) in layer {
        let Value::Object(overrides) = overrides else {
            continue;
        };
        match base.get_mut(&name) {
            Some(Value::Object(existing)) => existing.extend(overrides),
            _ => {
                base.insert(name, Value::Object(overrides));
            }
        }
    }
}

/// The built-in servers with each `lsp.json` layer applied in order; later layers win.
pub fn resolve_servers(layers: Vec<Map<String, Value>>) -> Result<Vec<ServerConfig>, String> {
    let mut merged = default_server_values();
    for layer in layers {
        merge_layer(&mut merged, layer);
    }
    let order: Vec<&str> = DEFAULT_SERVERS.iter().map(|server| server.name).collect();
    let mut servers = Vec::new();
    for (name, value) in merged {
        let mut server: ServerConfig =
            serde_json::from_value(value).map_err(|error| format!("lsp server '{name}': {error}"))?;
        server.name = name;
        if !server.disabled {
            servers.push(server);
        }
    }
    let rank = |server: &ServerConfig| order.iter().position(|name| *name == server.name).unwrap_or(usize::MAX);
    servers.sort_by_key(|server| (server.linter, rank(server)));
    Ok(servers)
}

/// Reads one `lsp.json` file; a missing file is an empty layer.
pub fn read_layer(path: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    };
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{} must contain a JSON object", path.display())),
        Err(error) => Err(format!("parsing {}: {error}", path.display())),
    }
}

/// The servers a workspace may run, with a notice for the status report when something was left out.
#[derive(Debug, Default)]
pub struct ServerSet {
    pub servers: Vec<ServerConfig>,
    pub notice: Option<String>,
    /// The top-level `idleTimeoutMs` of the last applied file that sets it; 0 never stops idle servers.
    pub idle_timeout_ms: Option<u64>,
}

/// The user's `lsp.json` (when there is a home), then the project's two files.
pub fn layer_paths(home: Option<&Path>, workspace: &Path) -> Vec<PathBuf> {
    home.map(|home| home.join(".agents").join("lsp.json"))
        .into_iter()
        .chain([workspace.join(".lsp.json"), workspace.join(".agents").join("lsp.json")])
        .collect()
}

/// The project's two files as layers. A file that cannot be read is an error only when it would
/// apply; an untrusted project's broken file is a notice, so it cannot disable the user's servers.
struct ProjectLayers {
    layers: Vec<Map<String, Value>>,
    notices: Vec<String>,
}

fn project_layers(workspace: &Path, trust_project: bool) -> Result<ProjectLayers, String> {
    let mut layers = Vec::new();
    let mut ignored = Vec::new();
    let mut notices = Vec::new();
    for path in [workspace.join(".lsp.json"), workspace.join(".agents").join("lsp.json")] {
        match (read_layer(&path), trust_project) {
            (Ok(layer), _) if layer.is_empty() => {}
            (Ok(layer), true) => layers.push(layer),
            (Ok(_), false) => ignored.push(path.display().to_string()),
            (Err(error), true) => return Err(error),
            (Err(error), false) => notices.push(format!("ignored project file: {error}")),
        }
    }
    if !ignored.is_empty() {
        notices.push(format!(
            "ignored {}: project files choose commands to run, so they apply only with RHO_TRUST_PROJECT=1; copy trusted servers to ~/.agents/lsp.json",
            ignored.join(" and ")
        ));
    }
    Ok(ProjectLayers { layers, notices })
}

/// The `idleTimeoutMs` of the last layer that sets a valid one; invalid values are skipped with a notice.
fn idle_timeout(layers: &[Map<String, Value>], notices: &mut Vec<String>) -> Option<u64> {
    let mut invalid = false;
    let mut found = None;
    for value in layers.iter().rev().filter_map(|layer| layer.get("idleTimeoutMs")) {
        match value.as_u64() {
            Some(ms) if found.is_none() => found = Some(ms),
            Some(_) => {}
            None => invalid = true,
        }
    }
    if invalid {
        notices.push("ignored idleTimeoutMs: it must be a whole number of milliseconds".into());
    }
    found
}

/// Built-in servers, then `~/.agents/lsp.json`, then the project's `.lsp.json` and
/// `.agents/lsp.json`. Project files choose which commands run, so they apply only when
/// `trust_project`; a top-level `"enabled": false` in any applied file turns every server off.
pub fn load_servers(home: Option<&Path>, workspace: &Path, trust_project: bool) -> Result<ServerSet, String> {
    let mut layers = Vec::new();
    if let Some(home) = home {
        layers.push(read_layer(&home.join(".agents").join("lsp.json"))?);
    }
    let ProjectLayers {
        layers: project,
        mut notices,
    } = project_layers(workspace, trust_project)?;
    layers.extend(project);
    if layers
        .iter()
        .any(|layer| layer.get("enabled") == Some(&Value::Bool(false)))
    {
        return Ok(ServerSet {
            servers: Vec::new(),
            notice: Some("language servers are turned off (\"enabled\": false in lsp.json)".into()),
            idle_timeout_ms: None,
        });
    }
    let idle_timeout_ms = idle_timeout(&layers, &mut notices);
    Ok(ServerSet {
        servers: resolve_servers(layers)?,
        notice: (!notices.is_empty()).then(|| notices.join("; ")),
        idle_timeout_ms,
    })
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
}

impl ServerConfig {
    pub fn handles(&self, path: &Path) -> bool {
        let Some(ext) = extension_of(path) else {
            return false;
        };
        self.extensions
            .iter()
            .chain(self.extension_to_language.keys())
            .any(|candidate| candidate.trim_start_matches('.').eq_ignore_ascii_case(&ext))
    }

    pub fn language_id(&self, path: &Path) -> String {
        let ext = extension_of(path).unwrap_or_default();
        let configured = self
            .extension_to_language
            .iter()
            .find(|(key, _)| key.trim_start_matches('.').eq_ignore_ascii_case(&ext))
            .map(|(_, language)| language.clone());
        configured.unwrap_or_else(|| {
            LANGUAGE_IDS
                .iter()
                .find(|(known, _)| *known == ext)
                .map_or(ext.clone(), |(_, id)| (*id).to_string())
        })
    }

    fn has_marker(&self, dir: &Path) -> bool {
        self.root_markers.iter().any(|marker| match marker.strip_prefix("*.") {
            Some(ext) => std::fs::read_dir(dir).is_ok_and(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.path().extension().is_some_and(|found| found == ext))
            }),
            None => dir.join(marker).exists(),
        })
    }

    /// The workspace when it holds a root marker (one server per monorepo), else the nearest marked
    /// ancestor of `file`; marker-less servers use the workspace.
    pub fn root_for(&self, file: &Path, workspace: &Path) -> Option<PathBuf> {
        if self.root_markers.is_empty() || (file.starts_with(workspace) && self.has_marker(workspace)) {
            return Some(workspace.to_path_buf());
        }
        let stop = if file.starts_with(workspace) {
            Some(workspace)
        } else {
            None
        };
        let mut dir = file.parent();
        while let Some(current) = dir {
            if self.has_marker(current) {
                return Some(current.to_path_buf());
            }
            if Some(current) == stop {
                break;
            }
            dir = current.parent();
        }
        None
    }
}

/// Resolves a command name against `path_var` (a `PATH`-style list); a name with a slash is taken
/// relative to `root`, where the server runs. The result is absolute: relative `PATH` entries mean
/// different directories to the check and the spawn, so they are skipped.
pub fn resolve_command(command: &str, path_var: Option<&std::ffi::OsStr>, root: &Path) -> Option<PathBuf> {
    if command.contains('/') {
        let path = root.join(command);
        return is_executable(&path).then_some(path);
    }
    std::env::split_paths(path_var?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(command))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file() || path.with_extension("exe").is_file()
    }
}

#[cfg(test)]
mod tests;
