use crate::permission::policy::{
    LoadedPolicy, Policy, build_policy, load_policy, parse_scope_from_str, project_config_path,
};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use crate::test_support::temp_dir;
use rho_test_support::EnvVarGuard;
use serde_json::json;
use std::path::Path;
use test_case::test_case;

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

const ALLOW_GIT_CARGO: &str = "[allow]\nbash = [\"git *\", \"cargo *\"]\n";
const ALLOW_GIT: &str = "[allow]\nbash = [\"git *\"]\n[deny]\nbash = [\"git push *\"]\n";
const ALLOW_CAT: &str = "[allow]\nbash = [\"cat *\"]\n[ask]\nbash = [\"cat secret*\"]\n";
const ASK_CAT: &str = "[ask]\nbash = [\"cat *\"]\n[deny]\nbash = [\"cat secret*\"]\n";
const MERGED_GLOBAL: &str = "[permission.bash]\n\"cargo *\" = \"allow\"\n\"git *\" = \"allow\"\n";
const MERGED_PROJECT: &str =
    "[permission.bash]\n\"cargo publish\" = { action = \"deny\", reason = \"publishing forbidden from repo\" }\n";

#[test_case(ALLOW_GIT_CARGO, None, "cargo test", Decision::Allow; "allow listed cargo")]
#[test_case(ALLOW_GIT_CARGO, None, "git status && cargo test", Decision::Allow; "allow chained listed")]
#[test_case(ALLOW_GIT_CARGO, None, "cargo test && npm publish", Decision::Ask; "ask chained unlisted")]
#[test_case(ALLOW_GIT, None, "git status", Decision::Allow; "allow git status")]
#[test_case(
    ALLOW_GIT,
    None,
    "git push origin main",
    Decision::Deny("denied by permission rule 'bash|git push *'".to_string());
    "deny beats allow"
)]
#[test_case(ALLOW_CAT, None, "cat notes.txt", Decision::Allow; "allow cat notes")]
#[test_case(ALLOW_CAT, None, "cat secret.txt", Decision::Ask; "ask beats allow")]
#[test_case(
    ASK_CAT,
    None,
    "cat secret.txt",
    Decision::Deny("denied by permission rule 'bash|cat secret*'".to_string());
    "deny beats ask"
)]
#[test_case(ASK_CAT, None, "cat notes.txt", Decision::Ask; "ask cat notes")]
#[test_case(MERGED_GLOBAL, Some(MERGED_PROJECT), "cargo test", Decision::Allow; "merged global cargo")]
#[test_case(MERGED_GLOBAL, Some(MERGED_PROJECT), "git status", Decision::Allow; "merged global git")]
#[test_case(
    MERGED_GLOBAL,
    Some(MERGED_PROJECT),
    "cargo publish",
    Decision::Deny("publishing forbidden from repo".to_string());
    "merged project deny"
)]
fn bash_rule_precedence(global: &str, project: Option<&str>, cmd: &str, expected: Decision) {
    let policy = bash_policy(global, project);
    assert_eq!(decide_with(&policy, "bash", json!({"command": cmd})), expected);
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

#[test_case("allow", Decision::Allow; "allow")]
#[test_case("ask", Decision::Ask; "ask")]
#[test_case("deny", Decision::Deny("because".to_string()); "deny")]
fn inline_table_rules_honor_their_action(action: &str, expected: Decision) {
    let toml = format!("[permission.bash]\n\"rm *\" = {{ action = \"{action}\", reason = \"because\" }}\n");
    let policy = build_policy(Some(parse_scope_from_str(&toml).unwrap()), None);

    assert_eq!(
        decide_with(&policy, "bash", json!({"command": "rm notes.txt"})),
        expected
    );
}

#[test]
fn inline_table_with_unknown_action_is_rejected() {
    let toml = "[permission.bash]\n\"rm *\" = { action = \"maybe\" }\n";

    assert!(parse_scope_from_str(toml).is_err());
}

#[test]
fn malformed_scope_fails_safely() {
    let malformed = "[permission\nbash = \"*\"]\n";
    assert!(parse_scope_from_str(malformed).is_err());
}

fn setup_hierarchical_dirs() -> (crate::test_support::TestDir, crate::test_support::TestDir) {
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

fn load_test_policy_with_env(global_dir: &std::path::Path, project_dir: &std::path::Path) -> LoadedPolicy {
    let _env = EnvVarGuard::set("RHO_HOME", &global_dir.to_string_lossy());
    load_policy(Some(project_dir))
}

#[test_case("global_cmd run", Decision::Allow; "global rule")]
#[test_case("project_cmd run", Decision::Allow; "project rule")]
#[test_case("unknown_cmd run", Decision::Ask; "unknown command")]
fn load_policy_discovers_project_and_global_hierarchies(cmd: &str, expected: Decision) {
    let (global_dir, project_dir) = setup_hierarchical_dirs();
    assert!(project_config_path(Some(&project_dir)).is_some());
    let policy = load_test_policy_with_env(&global_dir, &project_dir).policy;

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

#[test]
fn malformed_global_policy_warns_and_keeps_project_rules() {
    let (global_dir, project_dir) = setup_hierarchical_dirs();
    std::fs::write(global_dir.join("permission.toml"), "[permission\n").unwrap();
    let loaded = load_test_policy_with_env(&global_dir, &project_dir);

    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    assert!(loaded.warnings[0].contains("permission.toml"), "{:?}", loaded.warnings);
    assert_eq!(
        decide_with(&loaded.policy, "bash", json!({"command": "project_cmd run"})),
        Decision::Allow
    );
}

#[cfg(unix)]
#[test]
fn unreadable_project_policy_warns_instead_of_failing_silently() {
    use std::os::unix::fs::PermissionsExt;

    let (global_dir, project_dir) = setup_hierarchical_dirs();
    let project_file = project_dir.join(".rho/permission.toml");
    std::fs::set_permissions(&project_file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let loaded = load_test_policy_with_env(&global_dir, &project_dir);
    std::fs::set_permissions(&project_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    assert_eq!(
        decide_with(&loaded.policy, "bash", json!({"command": "global_cmd run"})),
        Decision::Allow
    );
}

#[test]
fn missing_policy_files_are_not_warnings() {
    let (global_dir, project_dir) = setup_hierarchical_dirs();
    std::fs::remove_file(global_dir.join("permission.toml")).unwrap();
    let loaded = load_test_policy_with_env(&global_dir, &project_dir);

    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
}

#[test]
fn load_policy_discovers_dot_config_rho_fallback() {
    let project_dir = crate::test_support::TestDir::new();
    let perm_file = project_dir.join(".config/rho/permission.toml");
    crate::test_support::write_file(&perm_file, "[permission.bash]\n\"fallback_cmd *\" = \"allow\"\n");

    assert_eq!(project_config_path(Some(&project_dir)), Some(perm_file));

    let policy = load_policy(Some(&project_dir)).policy;
    assert_eq!(
        decide_with(&policy, "bash", json!({"command": "fallback_cmd run"})),
        Decision::Allow
    );
}
