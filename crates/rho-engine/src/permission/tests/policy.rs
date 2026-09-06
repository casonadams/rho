use crate::permission::policy::{Policy, build_policy, load_policy, parse_scope_from_str, project_config_path};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rho-perm-policy-test-{}-{name}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ws() -> Option<&'static Path> {
    Some(Path::new("/ws"))
}

#[test]
fn allow_rule_approves_every_subcommand() {
    let scope = parse_scope_from_str("[allow]\nbash = [\"git *\", \"cargo *\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let check = |cmd: &str| {
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                working_dir: ws(),
            },
        )
    };
    assert_eq!(check("cargo test"), Decision::Allow);
    assert_eq!(check("git status && cargo test"), Decision::Allow);
    assert_eq!(check("cargo test && npm publish"), Decision::Ask);
}

#[test]
fn deny_rule_beats_allow_rule() {
    let scope = parse_scope_from_str("[allow]\nbash = [\"git *\"]\n[deny]\nbash = [\"git push *\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let check = |cmd: &str| {
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                working_dir: ws(),
            },
        )
    };
    assert_eq!(check("git status"), Decision::Allow);
    assert_eq!(
        check("git push origin main"),
        Decision::Deny("denied by permission rule 'bash|git push *'".to_string())
    );
}

#[test]
fn ask_rule_beats_allow_rule() {
    let scope = parse_scope_from_str("[allow]\nbash = [\"cat *\"]\n[ask]\nbash = [\"cat secret*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let check = |cmd: &str| {
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                working_dir: ws(),
            },
        )
    };
    assert_eq!(check("cat notes.txt"), Decision::Allow);
    assert_eq!(check("cat secret.txt"), Decision::Ask);
}

#[test]
fn deny_rule_beats_ask_rule() {
    let scope = parse_scope_from_str("[ask]\nbash = [\"cat *\"]\n[deny]\nbash = [\"cat secret*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let check = |cmd: &str| {
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                working_dir: ws(),
            },
        )
    };
    assert_eq!(
        check("cat secret.txt"),
        Decision::Deny("denied by permission rule 'bash|cat secret*'".to_string())
    );
    assert_eq!(check("cat notes.txt"), Decision::Ask);
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

    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "/tmp/notes.txt"}),
                working_dir: ws(),
            }
        ),
        Decision::Allow
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "local.env"}),
                working_dir: ws(),
            }
        ),
        Decision::Deny("do not access env secrets".to_string())
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": "rm -rf /tmp/junk"}),
                working_dir: ws(),
            }
        ),
        Decision::Deny("destructive command".to_string())
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": "cargo test --lib"}),
                working_dir: ws(),
            }
        ),
        Decision::Allow
    );
}

fn build_test_merged_policy() -> Policy {
    let global_toml = "[permission.bash]\n\"cargo *\" = \"allow\"\n\"git *\" = \"allow\"\n";
    let project_toml =
        "[permission.bash]\n\"cargo publish\" = { action = \"deny\", reason = \"publishing forbidden from repo\" }\n";
    let global_scope = parse_scope_from_str(global_toml).unwrap();
    let project_scope = parse_scope_from_str(project_toml).unwrap();
    build_policy(Some(global_scope), Some(project_scope))
}

#[test]
fn global_and_project_scope_merging() {
    let policy = build_test_merged_policy();
    let check = |cmd: &str| {
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                working_dir: ws(),
            },
        )
    };
    assert_eq!(check("cargo test"), Decision::Allow);
    assert_eq!(check("git status"), Decision::Allow);
    assert_eq!(
        check("cargo publish"),
        Decision::Deny("publishing forbidden from repo".to_string())
    );
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
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", global_dir);
    }
    let (p, healthy) = load_policy(Some(project_dir));
    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    assert!(healthy);
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
    let project_dir = temp_dir("dot_config_fallback");
    let dot_config_rho = project_dir.join(".config/rho");
    std::fs::create_dir_all(&dot_config_rho).unwrap();
    let perm_file = dot_config_rho.join("permission.toml");
    std::fs::write(&perm_file, "[permission.bash]\n\"fallback_cmd *\" = \"allow\"\n").unwrap();

    assert_eq!(project_config_path(Some(&project_dir)), Some(perm_file));

    let (policy, healthy) = load_policy(Some(&project_dir));
    assert!(healthy);
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": "fallback_cmd run"}),
                working_dir: Some(&project_dir),
            }
        ),
        Decision::Allow
    );
    let _ = std::fs::remove_dir_all(project_dir);
}
