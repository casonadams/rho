use crate::permission::policy::{Policy, build_policy, load_policy, parse_scope_from_str, project_config_path};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use crate::test_support::temp_dir;
use serde_json::json;
use std::path::{Path, PathBuf};

fn ws() -> Option<&'static Path> {
    Some(Path::new("/ws"))
}

fn decide_with(policy: &Policy, tool: &'static str, args: serde_json::Value) -> Decision {
    decide_tool_call(
        policy,
        EvalRequest {
            tool,
            args: &args,
            mcp_servers: &[],
            working_dir: ws(),
        },
    )
}

fn bash_policy(global: &str, project: Option<&str>) -> Policy {
    let global = parse_scope_from_str(global).unwrap();
    let project = project.map(|p| parse_scope_from_str(p).unwrap());
    build_policy(Some(global), project)
}

#[test]
fn bash_rule_precedence() {
    let allow_git_cargo = "[allow]\nbash = [\"git *\", \"cargo *\"]\n";
    let allow_git = "[allow]\nbash = [\"git *\"]\n[deny]\nbash = [\"git push *\"]\n";
    let allow_cat = "[allow]\nbash = [\"cat *\"]\n[ask]\nbash = [\"cat secret*\"]\n";
    let ask_cat = "[ask]\nbash = [\"cat *\"]\n[deny]\nbash = [\"cat secret*\"]\n";
    let merged_global = "[permission.bash]\n\"cargo *\" = \"allow\"\n\"git *\" = \"allow\"\n";
    let merged_project =
        "[permission.bash]\n\"cargo publish\" = { action = \"deny\", reason = \"publishing forbidden from repo\" }\n";
    let cases = [
        (allow_git_cargo, None, "cargo test", Decision::Allow),
        (allow_git_cargo, None, "git status && cargo test", Decision::Allow),
        (allow_git_cargo, None, "cargo test && npm publish", Decision::Ask),
        (allow_git, None, "git status", Decision::Allow),
        (
            allow_git,
            None,
            "git push origin main",
            Decision::Deny("denied by permission rule 'bash|git push *'".to_string()),
        ),
        (allow_cat, None, "cat notes.txt", Decision::Allow),
        (allow_cat, None, "cat secret.txt", Decision::Ask),
        (
            ask_cat,
            None,
            "cat secret.txt",
            Decision::Deny("denied by permission rule 'bash|cat secret*'".to_string()),
        ),
        (ask_cat, None, "cat notes.txt", Decision::Ask),
        (merged_global, Some(merged_project), "cargo test", Decision::Allow),
        (merged_global, Some(merged_project), "git status", Decision::Allow),
        (
            merged_global,
            Some(merged_project),
            "cargo publish",
            Decision::Deny("publishing forbidden from repo".to_string()),
        ),
    ];
    for (global, project, cmd, expected) in cases {
        let policy = bash_policy(global, project);
        assert_eq!(decide_with(&policy, "bash", json!({"command": cmd})), expected, "{cmd}");
    }
}

#[test]
fn permission_surface_tables_and_custom_deny_reason() {
    let toml = r#"
[permission.path]
"/tmp/*" = "allow"
"*.env*" = { action = "deny", reason = "do not access env secrets" }

[permission.bash]
"cargo test *" = "allow"
"rm -rf *" = { action = "deny", reason = "destructive command" }
"#;
    let scope = parse_scope_from_str(toml).unwrap();
    let policy = build_policy(Some(scope), None);

    let decide = |tool: &'static str, args: serde_json::Value| decide_with(&policy, tool, args);
    assert_eq!(decide("read", json!({"path": "/tmp/notes.txt"})), Decision::Allow);
    assert_eq!(
        decide("read", json!({"path": "local.env"})),
        Decision::Deny("do not access env secrets".to_string())
    );
    assert_eq!(
        decide("bash", json!({"command": "rm -rf /tmp/junk"})),
        Decision::Deny("destructive command".to_string())
    );
    assert_eq!(decide("bash", json!({"command": "cargo test --lib"})), Decision::Allow);
}

#[test]
fn malformed_scope_fails_safely() {
    let malformed = "[permission\nbash = \"*\"]\n";
    assert!(parse_scope_from_str(malformed).is_err());
}

fn setup_hierarchical_dirs() -> (PathBuf, PathBuf) {
    let global_dir = temp_dir("global");
    std::fs::write(
        global_dir.join("permission.toml"),
        "[permission.bash]\n\"global_cmd *\" = \"allow\"\n",
    )
    .unwrap();
    let project_dir = temp_dir("project");
    let dot_rho = project_dir.join(".rho");
    std::fs::create_dir_all(&dot_rho).unwrap();
    std::fs::write(
        dot_rho.join("permission.toml"),
        "[permission.bash]\n\"project_cmd *\" = \"allow\"\n",
    )
    .unwrap();
    (global_dir, project_dir)
}

fn load_test_policy_with_env(global_dir: &std::path::Path, project_dir: &std::path::Path) -> Policy {
    let _guard = super::ENV_LOCK.blocking_lock();
    unsafe {
        std::env::set_var("RHO_HOME", global_dir);
    }
    let p = load_policy(Some(project_dir));
    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    p
}

#[test]
fn load_policy_discovers_project_and_global_hierarchies() {
    let (global_dir, project_dir) = setup_hierarchical_dirs();
    assert!(project_config_path(Some(&project_dir)).is_some());
    let policy = load_test_policy_with_env(&global_dir, &project_dir);

    let cases = [
        ("global_cmd run", Decision::Allow),
        ("project_cmd run", Decision::Allow),
        ("unknown_cmd run", Decision::Ask),
    ];
    for (cmd, expected) in cases {
        let dec = decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                mcp_servers: &[],
                working_dir: Some(&project_dir),
            },
        );
        assert_eq!(dec, expected);
    }
    let _ = std::fs::remove_dir_all(global_dir);
    let _ = std::fs::remove_dir_all(project_dir);
}

#[test]
fn load_policy_discovers_dot_config_rho_fallback() {
    let project_dir = tempfile::tempdir().unwrap();
    let perm_file = project_dir.path().join(".config/rho/permission.toml");
    crate::test_support::write_file(&perm_file, "[permission.bash]\n\"fallback_cmd *\" = \"allow\"\n");

    assert_eq!(project_config_path(Some(project_dir.path())), Some(perm_file));

    let policy = load_policy(Some(project_dir.path()));
    assert_eq!(
        decide_with(&policy, "bash", json!({"command": "fallback_cmd run"})),
        Decision::Allow
    );
}
