use rho_harness_core::config::PluginConfig;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, Default)]
pub struct PluginEnvironment<'a> {
    pub cargo_bin_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
}

pub fn resolve_cargo_bin_dir() -> Option<PathBuf> {
    let install_root = std::env::var_os("CARGO_INSTALL_ROOT")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let cargo_home = std::env::var_os("CARGO_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let home = dirs::home_dir();
    resolve_cargo_bin_dir_from(install_root.as_deref(), cargo_home.as_deref(), home.as_deref())
}

pub fn resolve_cargo_bin_dir_from(
    install_root: Option<&Path>,
    cargo_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(root) = install_root.filter(|p| !p.as_os_str().is_empty()) {
        return Some(root.join("bin"));
    }
    if let Some(home) = cargo_home.filter(|p| !p.as_os_str().is_empty()) {
        return Some(home.join("bin"));
    }
    home_dir
        .filter(|h| !h.as_os_str().is_empty())
        .map(|h| h.join(".cargo").join("bin"))
}

pub fn expand_home(path: &Path, home_dir: Option<&Path>) -> PathBuf {
    if let Some(home) = home_dir {
        if path == Path::new("~") {
            return home.to_path_buf();
        }
        if let Ok(stripped) = path.strip_prefix("~/") {
            return home.join(stripped);
        }
        if let Ok(stripped) = path.strip_prefix("~") {
            return home.join(stripped);
        }
    }
    path.to_path_buf()
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c),
        }
    }
    normalized
}

pub fn is_in_cargo_bin(target: &Path, cargo_bin_dir: &Path) -> bool {
    if let (Ok(target_canon), Ok(bin_canon)) = (target.canonicalize(), cargo_bin_dir.canonicalize()) {
        return target_canon.parent() == Some(&bin_canon);
    }
    if let (Some(parent), Ok(bin_canon)) = (target.parent(), cargo_bin_dir.canonicalize())
        && let Ok(parent_canon) = parent.canonicalize()
    {
        return parent_canon == bin_canon;
    }
    let norm_target = normalize_path(target);
    let norm_bin = normalize_path(cargo_bin_dir);
    norm_target.parent() == Some(&norm_bin)
}

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn resolve_explicit_plugin_path(path: &Path, cargo_bin_dir: Option<&Path>, home_dir: Option<&Path>) -> PathBuf {
    let expanded = expand_home(path, home_dir);
    if expanded.is_absolute() {
        return expanded;
    }
    if let Some(bin_dir) = cargo_bin_dir {
        let candidate = bin_dir.join(&expanded);
        if candidate.is_file() {
            return candidate;
        }
    }
    expanded
}

fn find_in_system_path(cmd: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_command_plugin_path(cmd: &str, cargo_bin_dir: Option<&Path>, home_dir: Option<&Path>) -> PathBuf {
    let trimmed = cmd.trim();
    if trimmed.contains('/') || trimmed.contains('\\') {
        return expand_home(Path::new(trimmed), home_dir);
    }
    if let Some(candidate) = cargo_bin_dir.map(|b| b.join(trimmed)).filter(|c| c.is_file()) {
        return candidate;
    }
    if let Some(candidate) = find_in_system_path(trimmed) {
        return candidate;
    }
    cargo_bin_dir
        .map(|b| b.join(trimmed))
        .unwrap_or_else(|| PathBuf::from(trimmed))
}

pub fn resolve_plugin_binary_path(
    plugin: &PluginConfig,
    cargo_bin_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    if !plugin.path.as_os_str().is_empty() {
        return Some(resolve_explicit_plugin_path(&plugin.path, cargo_bin_dir, home_dir));
    }

    plugin
        .command
        .as_deref()
        .map(|cmd| resolve_command_plugin_path(cmd, cargo_bin_dir, home_dir))
}

#[cfg(test)]
#[path = "paths/tests.rs"]
mod tests;
