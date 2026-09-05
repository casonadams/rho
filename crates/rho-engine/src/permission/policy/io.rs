use super::eval::build_policy;
use super::model::{Policy, ScopeRules};
use super::parse::parse_scope_from_str;
use std::path::{Path, PathBuf};

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

pub fn config_is_healthy() -> bool {
    let cwd = std::env::current_dir().ok();
    let (_, healthy) = load_policy(cwd.as_deref());
    healthy
}

pub fn read_scope_file(path: &Path) -> Result<ScopeRules, String> {
    if !path.exists() {
        return Ok(ScopeRules::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_scope_from_str(&raw)
}

pub fn project_config_path(cwd: Option<&Path>) -> Option<PathBuf> {
    let cwd = cwd?;
    let dot_rho = cwd.join(".rho/permission.toml");
    if dot_rho.exists() {
        return Some(dot_rho);
    }
    let dot_config = cwd.join(".config/rho/permission.toml");
    if dot_config.exists() {
        return Some(dot_config);
    }
    None
}

pub fn target_config_path(cwd: Option<&Path>) -> Option<PathBuf> {
    project_config_path(cwd).or_else(config_path)
}

pub fn load_policy(cwd: Option<&Path>) -> (Policy, bool) {
    let global_path = config_path();
    let global = global_path.as_deref().map(read_scope_file);
    let project_path = project_config_path(cwd);
    let project = project_path.as_deref().map(read_scope_file);

    let healthy = global.as_ref().is_none_or(Result::is_ok) && project.as_ref().is_none_or(Result::is_ok);
    let global_scope = global.and_then(Result::ok);
    let project_scope = project.and_then(Result::ok);
    let policy = build_policy(global_scope, project_scope);
    (policy, healthy)
}

pub fn save_allow_rule(path: &Path, tool: &str, pattern: &str) -> Result<(), String> {
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
