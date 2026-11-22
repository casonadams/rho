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

/// Name, command, args, file extensions, and root markers.
type DefaultServer = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

const DEFAULT_SERVERS: &[DefaultServer] = &[
    (
        "rust-analyzer",
        "rust-analyzer",
        &[],
        &["rs"],
        &["Cargo.toml", "rust-project.json"],
    ),
    ("gopls", "gopls", &[], &["go"], &["go.work", "go.mod"]),
    (
        "typescript-language-server",
        "typescript-language-server",
        &["--stdio"],
        &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
        &["tsconfig.json", "jsconfig.json", "package.json"],
    ),
    (
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
    (
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
    (
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
    ("zls", "zls", &[], &["zig"], &["build.zig"]),
    (
        "lua-language-server",
        "lua-language-server",
        &[],
        &["lua"],
        &[".luarc.json", ".luarc.jsonc"],
    ),
    (
        "jdtls",
        "jdtls",
        &[],
        &["java"],
        &["pom.xml", "build.gradle", "build.gradle.kts"],
    ),
    (
        "kotlin-language-server",
        "kotlin-language-server",
        &[],
        &["kt", "kts"],
        &["settings.gradle", "settings.gradle.kts", "build.gradle.kts"],
    ),
    ("ruby-lsp", "ruby-lsp", &[], &["rb"], &["Gemfile"]),
    ("sourcekit-lsp", "sourcekit-lsp", &[], &["swift"], &["Package.swift"]),
    (
        "haskell-language-server",
        "haskell-language-server-wrapper",
        &["--lsp"],
        &["hs"],
        &["stack.yaml", "cabal.project", "*.cabal", "hie.yaml"],
    ),
    ("ocamllsp", "ocamllsp", &[], &["ml", "mli"], &["dune-project", "*.opam"]),
    ("csharp-ls", "csharp-ls", &[], &["cs"], &["*.sln", "*.csproj"]),
    ("elixir-ls", "elixir-ls", &[], &["ex", "exs"], &["mix.exs"]),
    (
        "intelephense",
        "intelephense",
        &["--stdio"],
        &["php"],
        &["composer.json"],
    ),
    (
        "bash-language-server",
        "bash-language-server",
        &["start"],
        &["sh", "bash"],
        &[],
    ),
    ("terraform-ls", "terraform-ls", &["serve"], &["tf", "tfvars"], &[]),
    ("nil", "nil", &[], &["nix"], &["flake.nix"]),
    ("taplo", "taplo", &["lsp", "stdio"], &["toml"], &[]),
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
        .map(|(name, command, args, extensions, markers)| {
            let value = json!({
                "command": command,
                "args": args,
                "extensions": extensions,
                "rootMarkers": markers,
            });
            ((*name).to_string(), value)
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
    let order: Vec<&str> = DEFAULT_SERVERS.iter().map(|server| server.0).collect();
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

/// `lsp.json` locations from lowest to highest precedence.
pub fn layer_paths(home: Option<&Path>, workspace: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join(".agents").join("lsp.json"));
    }
    paths.push(workspace.join(".lsp.json"));
    paths.push(workspace.join(".agents").join("lsp.json"));
    paths
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

/// Resolves a command name against `path_var` (a `PATH`-style list); names with a slash are used
/// as given when they exist.
pub fn resolve_command(command: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if command.contains('/') {
        let path = PathBuf::from(command);
        return is_executable(&path).then_some(path);
    }
    std::env::split_paths(path_var?)
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
