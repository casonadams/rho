use crate::permission::matcher::wildcard_match;
use crate::permission::path::{
    extract_mcp_path, extract_mcp_targets, extract_tool_path, is_credential_path, is_infrastructure_read,
    is_path_outside_working_dir, is_protected_path, is_safe_system_path, path_policy_values,
};
use rho_test_support::EnvVarGuard;
use serde_json::json;
use std::path::{Path, PathBuf};
use test_case::test_case;

type Check = fn(&str, Option<&Path>) -> bool;

#[test]
fn path_module_system_safety() {
    assert!(is_safe_system_path("/dev/null") && is_safe_system_path("/dev/stderr"));
    assert!(!is_safe_system_path("/tmp/foo"));
}

#[test_case(is_path_outside_working_dir, &["src/main.rs", "src/../src/lib.rs", "/dev/null"], false; "inside working dir")]
#[test_case(
    is_path_outside_working_dir,
    &["/etc/passwd", "../sibling/file", "a/../../etc/passwd"],
    true;
    "outside working dir"
)]
#[test_case(
    is_protected_path,
    &[".git/config", ".GIT/config", "src/.Git/HEAD", ".RHO/permission.toml", "src/../.git/config"],
    true;
    "protected vcs and rho dirs"
)]
#[test_case(is_protected_path, &["src/git.rs"], false; "git named source file")]
#[test_case(
    is_protected_path,
    &[".env", ".Env.local", ".config/rho/permission.toml", "/ws/.config/rho/permission.toml"],
    true;
    "protected env and config"
)]
fn working_dir_containment_and_protected_paths(check: Check, paths: &[&str], expected: bool) {
    let ws = Some(Path::new("/ws"));
    for &path in paths {
        assert_eq!(check(path, ws), expected, "{path}");
    }
}

#[test]
fn path_module_tool_extraction() {
    let ws = Path::new("/ws");
    assert!(path_policy_values("src/main.rs", Some(ws)).contains(&"src/main.rs".to_string()));

    let tool_args = json!({"path": "src/main.rs", "old_text": "foo"});
    assert_eq!(extract_tool_path("read", &tool_args), Some("src/main.rs".to_string()));
    assert_eq!(extract_tool_path("lsp", &tool_args), Some("src/main.rs".to_string()));
    assert_eq!(extract_tool_path("bash", &tool_args), None);
}

#[test]
fn path_module_mcp_extraction() {
    let mcp_args = json!({"server": "playwright", "tool": "navigate", "arguments": {"path": "/tmp/test.html"}});
    assert_eq!(extract_mcp_path(&mcp_args), Some("/tmp/test.html".to_string()));
    assert_eq!(
        extract_mcp_targets(&mcp_args),
        vec!["playwright:navigate", "playwright"]
    );
}

#[test]
fn quoted_paths_resolve_like_unquoted_paths() {
    let ws = Path::new("/ws");
    assert!(is_path_outside_working_dir("\"/etc/passwd\"", Some(ws)));
    assert!(is_path_outside_working_dir("'/etc/passwd'", Some(ws)));
    assert!(!is_path_outside_working_dir("\"src/main.rs\"", Some(ws)));
    assert_eq!(
        path_policy_values("\"src/main.rs\"", Some(ws)),
        path_policy_values("src/main.rs", Some(ws))
    );
}

#[test]
fn new_file_under_symlinked_parent_is_outside_when_target_is() {
    let root_dir = crate::test_support::TestDir::new();
    let outside_dir = crate::test_support::TestDir::new();
    let root = root_dir.canonicalize().unwrap();
    let outside = outside_dir.canonicalize().unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();

    assert!(is_path_outside_working_dir("linked/new.txt", Some(&root)));
    assert_eq!(
        path_policy_values("linked/new.txt", Some(&root)),
        vec![outside.join("new.txt").display().to_string()]
    );
}

#[test]
fn parent_segments_cannot_satisfy_workspace_glob_rules() {
    let ws = Path::new("/ws");
    let matches_src_glob = |raw: &str| {
        path_policy_values(raw, Some(ws))
            .iter()
            .any(|value| wildcard_match("src/*", value))
    };
    assert!(matches_src_glob("src/lib.rs"));
    assert!(!matches_src_glob("src/../../etc/passwd"));
    assert!(!matches_src_glob("src/../.git/config"));
}

#[test]
fn credential_store_is_identified_by_resolved_path() {
    let (config_dir, _env) = super::config_home();
    let auth = config_dir.join("auth.json");
    assert!(is_credential_path(&auth.display().to_string(), None));
    assert!(is_credential_path(
        &format!("{}/./auth.json", config_dir.display()),
        None
    ));
    assert!(!is_infrastructure_read("read", &auth.display().to_string(), None));
    let prompt = config_dir.join("prompts/review.md").display().to_string();
    assert!(is_infrastructure_read("read", &prompt, None));
    assert!(!is_infrastructure_read("write", &prompt, None));
}

#[test]
fn path_values_carry_absolute_then_cwd_relative() {
    let ws = PathBuf::from("/ws");
    assert_eq!(
        path_policy_values("/ws/src/a.rs", Some(&ws)),
        vec!["/ws/src/a.rs".to_string(), "src/a.rs".to_string()]
    );
}

#[test]
fn credential_store_follows_rust_ai_home_config_dir() {
    let dir = crate::test_support::TestDir::new();
    let auth = dir.join("auth.json");
    std::fs::write(&auth, "{}").unwrap();
    let _env = EnvVarGuard::set("RUST_AI_HOME", &dir.to_string_lossy());
    assert!(is_credential_path(&auth.display().to_string(), None));
}

#[test]
fn leading_env_variables_resolve_before_the_credential_check() {
    let dir = crate::test_support::TestDir::new();
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    let _env = EnvVarGuard::set("RHO_HOME", &dir.to_string_lossy());
    let hits = ["$RHO_HOME/auth.json", "${RHO_HOME}/auth.json"].map(|raw| is_credential_path(raw, None));
    let lookalikes = ["$RHO_HOME_X/auth.json", "$RHO_HOMEauth.json", "x$RHO_HOME/auth.json"]
        .map(|raw| is_credential_path(raw, None));
    assert_eq!(hits, [true, true]);
    assert_eq!(lookalikes, [false, false, false]);
}
