use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn make_plugin(command: Option<&str>, path: &str) -> PluginConfig {
    PluginConfig {
        command: command.map(ToString::to_string),
        path: PathBuf::from(path),
        ..Default::default()
    }
}

#[test]
fn test_dedup_empty_plugins() {
    let plugins = BTreeMap::new();
    let candidate = PluginCandidate {
        name: "rho-plugin-permission".to_string(),
        command: "rho-plugin-permission".to_string(),
        path: PathBuf::from("/bin/rho-plugin-permission"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert!(res.is_ok());
}

#[test]
fn test_dedup_distinct_plugin() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "rho-plugin-git".to_string(),
        make_plugin(Some("rho-plugin-git"), "/bin/rho-plugin-git"),
    );

    let candidate = PluginCandidate {
        name: "rho-plugin-shell".to_string(),
        command: "rho-plugin-shell".to_string(),
        path: PathBuf::from("/bin/rho-plugin-shell"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert!(res.is_ok());
}

#[test]
fn test_dedup_exact_name_collision() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "rho-plugin-git".to_string(),
        make_plugin(Some("rho-plugin-git"), "/bin/rho-plugin-git"),
    );

    let candidate = PluginCandidate {
        name: "rho-plugin-git".to_string(),
        command: "rho-plugin-git".to_string(),
        path: PathBuf::from("/bin/rho-plugin-git"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert_eq!(res, Err(DuplicatePluginError::Name("rho-plugin-git".to_string())));

    let candidate_force = PluginCandidate {
        name: "rho-plugin-git".to_string(),
        command: "rho-plugin-git".to_string(),
        path: PathBuf::from("/bin/rho-plugin-git"),
        force: true,
    };
    let res_force = validate_no_duplicates(&plugins, &candidate_force);
    assert!(res_force.is_ok());
}

#[test]
fn test_dedup_prefix_normalized_name_collision() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "permission".to_string(),
        make_plugin(Some("rho-plugin-permission"), "/bin/rho-plugin-permission"),
    );

    let candidate = PluginCandidate {
        name: "rho-plugin-permission".to_string(),
        command: "rho-plugin-permission".to_string(),
        path: PathBuf::from("/bin/rho-plugin-permission"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert_eq!(res, Err(DuplicatePluginError::Name("permission".to_string())));

    let candidate_force = PluginCandidate {
        name: "rho-plugin-permission".to_string(),
        command: "rho-plugin-permission".to_string(),
        path: PathBuf::from("/bin/rho-plugin-permission"),
        force: true,
    };
    let res_force = validate_no_duplicates(&plugins, &candidate_force);
    assert!(res_force.is_ok());
}

#[test]
fn test_dedup_command_collision_with_other_plugin() {
    let plugins = BTreeMap::from([(
        "custom-git".to_string(),
        make_plugin(Some("git-helper"), "/bin/git-helper"),
    )]);
    let candidate = PluginCandidate {
        name: "other-git".to_string(),
        command: "git-helper".to_string(),
        path: PathBuf::from("/bin/other-helper"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert_eq!(
        res,
        Err(DuplicatePluginError::Command {
            existing_plugin: "custom-git".to_string(),
            command: "git-helper".to_string()
        })
    );

    let candidate_force = PluginCandidate {
        name: "other-git".to_string(),
        command: "git-helper".to_string(),
        path: PathBuf::from("/bin/other-helper"),
        force: true,
    };
    assert!(validate_no_duplicates(&plugins, &candidate_force).is_err());
}

#[test]
fn test_dedup_path_collision_with_other_plugin() {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "plugin-a".to_string(),
        make_plugin(Some("cmd-a"), "/usr/local/bin/shared-tool"),
    );

    let candidate = PluginCandidate {
        name: "plugin-b".to_string(),
        command: "cmd-b".to_string(),
        path: PathBuf::from("/usr/local/bin/shared-tool"),
        force: false,
    };
    let res = validate_no_duplicates(&plugins, &candidate);
    assert_eq!(
        res,
        Err(DuplicatePluginError::Path {
            existing_plugin: "plugin-a".to_string(),
            path: PathBuf::from("/usr/local/bin/shared-tool"),
        })
    );
}

#[test]
fn test_dedup_cmd_matches_filename() {
    let plugins = BTreeMap::from([("tool-one".to_string(), make_plugin(None, "/usr/local/bin/my-tool"))]);
    let candidate = PluginCandidate {
        name: "tool-two".to_string(),
        command: "my-tool".to_string(),
        path: PathBuf::new(),
        force: false,
    };
    assert!(matches!(
        validate_no_duplicates(&plugins, &candidate),
        Err(DuplicatePluginError::Command { .. })
    ));
}

#[test]
fn test_dedup_path_matches_cmd_or_exact() {
    let plugins2 = BTreeMap::from([("tool-three".to_string(), make_plugin(Some("runner"), ""))]);
    let candidate2 = PluginCandidate {
        name: "tool-four".to_string(),
        command: "other".to_string(),
        path: PathBuf::from("/opt/bin/runner"),
        force: false,
    };
    assert!(matches!(
        validate_no_duplicates(&plugins2, &candidate2),
        Err(DuplicatePluginError::Command { .. })
    ));

    let plugins3 = BTreeMap::from([("tool-five".to_string(), make_plugin(Some("/exact/path/runner"), ""))]);
    let candidate3 = PluginCandidate {
        name: "tool-six".to_string(),
        command: "other".to_string(),
        path: PathBuf::from("/exact/path/runner"),
        force: false,
    };
    assert!(matches!(
        validate_no_duplicates(&plugins3, &candidate3),
        Err(DuplicatePluginError::Command { .. })
    ));
}
