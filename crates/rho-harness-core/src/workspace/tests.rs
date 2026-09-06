use super::*;

#[test]
fn excludes_configured_paths_from_mutations() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    let excluded = root.join(".rho");
    std::fs::create_dir_all(&excluded).unwrap();
    let workspace = Workspace::with_exclusions(&root, [&excluded]);
    assert!(!workspace.can_mutate(".rho/config.toml"));
    assert!(workspace.can_mutate("src/lib.rs"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolves_relative_paths_from_fixed_root() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let workspace = Workspace::new(&root);
    assert_eq!(workspace.resolve("src/lib.rs"), Some(canonical_root.join("src/lib.rs")));
    assert_eq!(workspace.resolve(" "), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolves_home_paths_when_env_present() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    let workspace = Workspace::new(&root);
    if let Ok(home) = std::env::var("HOME") {
        assert_eq!(workspace.resolve("~"), Some(PathBuf::from(&home)));
        assert_eq!(workspace.resolve("~/test"), Some(PathBuf::from(&home).join("test")));
    }
}

#[test]
fn protects_git_directory() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("subdir")).unwrap();
    let workspace = Workspace::new(&root);
    for path in [".git/config", "subdir/../.git/config"] {
        assert!(workspace.is_protected(path));
        assert!(!workspace.can_mutate(path));
    }
    assert!(workspace.is_protected(&root.join(".git/config").display().to_string()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_escape_outside_workspace() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let workspace = Workspace::new(&root);
    assert!(!workspace.is_within("../outside.txt"));
    std::fs::remove_dir_all(root).unwrap();
}
