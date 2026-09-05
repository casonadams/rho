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
