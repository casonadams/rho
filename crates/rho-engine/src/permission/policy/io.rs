use super::eval::build_policy;
use super::model::{Policy, ScopeRules};
use super::parse::parse_scope_from_str;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const PROTECTED_CONFIG_FILES: [&str; 3] = ["permission.toml", "config.toml", "auth.json"];
const DEFAULT_CONFIG_MODE: u32 = 0o644;

pub struct LoadedPolicy {
    pub policy: Policy,
    pub warnings: Vec<String>,
}

pub use rho_harness_core::config::config_dir;

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("permission.toml"))
}

pub fn read_scope_file(path: &Path) -> Result<ScopeRules, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(ScopeRules::default()),
        Err(error) => return Err(error.to_string()),
    };
    parse_scope_from_str(&raw)
}

pub fn project_policy_candidates(cwd: &Path) -> [PathBuf; 2] {
    [
        cwd.join(".rho/permission.toml"),
        cwd.join(".config/rho/permission.toml"),
    ]
}

pub fn project_config_path(cwd: Option<&Path>) -> Option<PathBuf> {
    project_policy_candidates(cwd?).into_iter().find(|path| path.exists())
}

pub fn protected_config_files(cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = config_dir()
        .map(|dir| PROTECTED_CONFIG_FILES.iter().map(|name| dir.join(name)).collect())
        .unwrap_or_default();
    if let Some(cwd) = cwd {
        files.extend(project_policy_candidates(cwd));
    }
    files
}

pub fn target_config_path() -> Option<PathBuf> {
    config_path()
}

pub fn load_policy(cwd: Option<&Path>) -> LoadedPolicy {
    let mut warnings = Vec::new();
    let global = config_path().and_then(|path| scope_or_warn(&path, &mut warnings));
    let project = project_config_path(cwd).and_then(|path| scope_or_warn(&path, &mut warnings));
    LoadedPolicy {
        policy: build_policy(global, project),
        warnings,
    }
}

fn scope_or_warn(path: &Path, warnings: &mut Vec<String>) -> Option<ScopeRules> {
    match read_scope_file(path) {
        Ok(scope) => Some(scope),
        Err(error) => {
            warnings.push(format!("Ignoring permission rules in {}: {error}", path.display()));
            None
        }
    }
}

pub fn save_allow_rule(path: &Path, tool: &str, pattern: &str) -> Result<(), String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    let mut doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("permission.toml is malformed: {error}"))?;

    let perm_table = ensure_permission_table(&mut doc)?;
    let surface_item = perm_table
        .entry(tool)
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));

    if let Some(table) = surface_item.as_table_mut() {
        table[pattern] = toml_edit::value("allow");
    } else if let Some(val) = surface_item.as_value_mut() {
        if pattern == "*" {
            *val = toml_edit::Value::from("allow");
        } else {
            let mut new_table = toml_edit::Table::new();
            new_table[pattern] = toml_edit::value("allow");
            *surface_item = toml_edit::Item::Table(new_table);
        }
    }
    let mode = config_file_mode(path);
    rho_harness_core::fs::write_atomic(path, doc.to_string().as_bytes(), mode).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn config_file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map_or(DEFAULT_CONFIG_MODE, |meta| meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn config_file_mode(_path: &Path) -> u32 {
    DEFAULT_CONFIG_MODE
}

fn ensure_permission_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table, String> {
    if doc.get("permission").is_none_or(toml_edit::Item::is_none) {
        doc["permission"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["permission"]
        .as_table_mut()
        .ok_or_else(|| "[permission] in permission.toml is not a table".to_string())
}
