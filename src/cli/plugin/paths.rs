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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_cargo_bin_dir_precedence() {
        let install_root = Path::new("/custom/install");
        let cargo_home = Path::new("/custom/cargo");
        let home = Path::new("/users/test");

        let resolved = resolve_cargo_bin_dir_from(Some(install_root), Some(cargo_home), Some(home));
        assert_eq!(resolved, Some(PathBuf::from("/custom/install/bin")));

        let resolved = resolve_cargo_bin_dir_from(None, Some(cargo_home), Some(home));
        assert_eq!(resolved, Some(PathBuf::from("/custom/cargo/bin")));

        let resolved = resolve_cargo_bin_dir_from(None, None, Some(home));
        assert_eq!(resolved, Some(PathBuf::from("/users/test/.cargo/bin")));

        let resolved = resolve_cargo_bin_dir_from(None, None, None);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_cargo_bin_dir_empty_strings() {
        let empty = Path::new("");
        let home = Path::new("/users/test");

        let resolved = resolve_cargo_bin_dir_from(Some(empty), Some(empty), Some(home));
        assert_eq!(resolved, Some(PathBuf::from("/users/test/.cargo/bin")));
    }

    #[test]
    fn test_expand_home() {
        let home = Path::new("/users/test");
        assert_eq!(
            expand_home(Path::new("~/tools/bin"), Some(home)),
            PathBuf::from("/users/test/tools/bin")
        );
        assert_eq!(expand_home(Path::new("~"), Some(home)), PathBuf::from("/users/test"));
        assert_eq!(
            expand_home(Path::new("/etc/hosts"), Some(home)),
            PathBuf::from("/etc/hosts")
        );
        assert_eq!(
            expand_home(Path::new("~/tools/bin"), None),
            PathBuf::from("~/tools/bin")
        );
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(normalize_path(Path::new("/a/b/../../c")), PathBuf::from("/c"));
    }

    #[test]
    fn test_is_in_cargo_bin_lexical() {
        let cargo_bin = Path::new("/users/test/.cargo/bin");

        assert!(is_in_cargo_bin(
            Path::new("/users/test/.cargo/bin/rho-plugin-git"),
            cargo_bin
        ));
        assert!(!is_in_cargo_bin(
            Path::new("/users/test/.cargo/bin/../system/cat"),
            cargo_bin
        ));
        assert!(!is_in_cargo_bin(Path::new("/usr/local/bin/rho-plugin-git"), cargo_bin));
        assert!(!is_in_cargo_bin(cargo_bin, cargo_bin));
    }

    #[test]
    fn test_is_in_cargo_bin_filesystem() {
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let plugin_file = bin_dir.join("test-plugin");
        fs::write(&plugin_file, "binary").unwrap();

        let outside_file = temp.path().join("outside-plugin");
        fs::write(&outside_file, "binary").unwrap();

        assert!(is_in_cargo_bin(&plugin_file, &bin_dir));
        assert!(!is_in_cargo_bin(&outside_file, &bin_dir));
    }

    #[test]
    fn test_resolve_plugin_binary_path_path_field() {
        let home = Path::new("/users/test");
        let cfg = PluginConfig {
            path: PathBuf::from("~/bin/custom"),
            ..Default::default()
        };

        let resolved = resolve_plugin_binary_path(&cfg, None, Some(home));
        assert_eq!(resolved, Some(PathBuf::from("/users/test/bin/custom")));
    }

    #[test]
    fn test_resolve_plugin_binary_path_in_cargo_bin() {
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let binary = bin_dir.join("my-plugin");
        fs::write(&binary, "executable").unwrap();

        let cfg = PluginConfig {
            command: Some("my-plugin".to_string()),
            ..Default::default()
        };

        let resolved = resolve_plugin_binary_path(&cfg, Some(&bin_dir), None);
        assert_eq!(resolved, Some(binary));
    }

    #[test]
    fn test_resolve_plugin_binary_path_fallback() {
        let bin_dir = Path::new("/virtual/cargo/bin");
        let cfg = PluginConfig {
            command: Some("missing-plugin".to_string()),
            ..Default::default()
        };

        let resolved = resolve_plugin_binary_path(&cfg, Some(bin_dir), None);
        assert_eq!(resolved, Some(PathBuf::from("/virtual/cargo/bin/missing-plugin")));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_executable_unix() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempdir().unwrap();
        let file = temp.path().join("exec_test");
        fs::write(&file, "test").unwrap();

        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file, perms).unwrap();
        assert!(!is_executable(&file));

        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&file, perms).unwrap();
        assert!(is_executable(&file));
    }
}
