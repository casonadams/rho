use super::*;
use std::collections::BTreeMap;

fn executable(path: &Path) {
    std::fs::write(path, b"fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn discovery_is_sorted_deduplicated_and_informational() {
    let root = std::env::temp_dir().join(format!("rho_discovery_{}", uuid::Uuid::new_v4()));
    let global = root.join("config/plugins");
    let workspace = root.join("project/.rho/plugins");
    let path_dir = root.join("path");
    std::fs::create_dir_all(&global).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&path_dir).unwrap();
    executable(&global.join("rho-plugin-zed"));
    executable(&path_dir.join("rho-plugin-alpha"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(path_dir.join("rho-plugin-alpha"), workspace.join("rho-plugin-alpha")).unwrap();

    let discovery = PluginLoader::discover_from(
        &root.join("config"),
        Some(&root.join("project")),
        DiscoveryPaths {
            cargo_bin: None,
            path_dirs: std::slice::from_ref(&path_dir),
        },
    )
    .unwrap();
    let names: Vec<_> = discovery
        .candidates
        .iter()
        .map(|candidate| candidate.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["rho-plugin-alpha", "rho-plugin-zed"]);
    #[cfg(unix)]
    assert_eq!(discovery.candidates[0].source, DiscoverySource::WorkspaceDirectory);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn only_configuration_creates_eligible_candidates() {
    let root = std::env::temp_dir().join(format!("rho_activation_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("bin")).unwrap();
    executable(&root.join("bin/rho-plugin-configured"));
    executable(&root.join("bin/rho-plugin-absolute"));
    executable(&root.join("bin/rho-plugin-ignored"));
    let configured = BTreeMap::from([
        (
            "absolute".to_string(),
            PluginConfig {
                path: root.join("bin/rho-plugin-absolute"),
                package: None,
                replaces: Default::default(),
                ..Default::default()
            },
        ),
        (
            "configured".to_string(),
            PluginConfig {
                path: PathBuf::from("bin/../bin/rho-plugin-configured"),
                package: None,
                replaces: Default::default(),
                ..Default::default()
            },
        ),
    ]);
    let candidates = PluginLoader::configured_candidates(&root, &configured);
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.status == ConfiguredStatus::Eligible)
    );
    assert!(candidates.iter().all(|candidate| candidate.path.is_absolute()));
    assert!(
        candidates
            .iter()
            .all(|candidate| !candidate.path.to_string_lossy().contains("ignored"))
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_candidate_reports_missing_and_non_executable_paths() {
    let root = std::env::temp_dir().join(format!("rho_activation_invalid_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("plain"), b"fixture").unwrap();
    let configured = BTreeMap::from([
        (
            "missing".to_string(),
            PluginConfig {
                path: "missing".into(),
                package: None,
                replaces: Default::default(),
                ..Default::default()
            },
        ),
        (
            "plain".to_string(),
            PluginConfig {
                path: "plain".into(),
                package: None,
                replaces: Default::default(),
                ..Default::default()
            },
        ),
    ]);
    let candidates = PluginLoader::configured_candidates(&root, &configured);
    assert_eq!(candidates[0].status, ConfiguredStatus::Missing);
    #[cfg(unix)]
    assert_eq!(candidates[1].status, ConfiguredStatus::NotExecutable);
    std::fs::remove_dir_all(root).unwrap();
}
