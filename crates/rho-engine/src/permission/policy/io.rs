use super::eval::build_policy;
use super::model::{Policy, ScopeRules};
use super::parse::parse_scope_from_str;
use std::path::{Path, PathBuf};

const PROTECTED_CONFIG_FILES: [&str; 3] = ["permission.toml", "config.toml", "auth.json"];

pub fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("RHO_HOME") {
        return Some(PathBuf::from(dir));
    }
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".config/rho"))
}

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("permission.toml"))
}

pub fn read_scope_file(path: &Path) -> Result<ScopeRules, String> {
    if !path.exists() {
        return Ok(ScopeRules::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
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

pub fn load_policy(cwd: Option<&Path>) -> Policy {
    let global_path = config_path();
    let global = global_path.as_deref().map(read_scope_file);
    let project_path = project_config_path(cwd);
    let project = project_path.as_deref().map(read_scope_file);

    let global_scope = global.and_then(Result::ok);
    let project_scope = project.and_then(Result::ok);
    build_policy(global_scope, project_scope)
}

pub fn save_allow_rule(path: &Path, tool: &str, pattern: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let raw = std::fs::read_to_string(path).unwrap_or_default();
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
    std::fs::write(path, doc.to_string()).map_err(|error| error.to_string())
}

fn ensure_permission_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table, String> {
    if doc.get("permission").is_none_or(toml_edit::Item::is_none) {
        doc["permission"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["permission"]
        .as_table_mut()
        .ok_or_else(|| "[permission] in permission.toml is not a table".to_string())
}
