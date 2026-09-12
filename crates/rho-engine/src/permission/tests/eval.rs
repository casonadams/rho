use crate::permission::eval::ask_drafts;
use crate::permission::policy::{Policy, build_policy, parse_scope_from_str, save_allow_rule};
use crate::permission::suggest::{canonical_tool, match_input, suggested_rule};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use serde_json::json;
use std::path::{Path, PathBuf};

fn ws() -> Option<&'static Path> {
    Some(Path::new("/ws"))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rho-perm-test-{}-{name}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn check_eval(policy: &Policy, tool: &'static str, args: serde_json::Value) -> Decision {
    decide_tool_call(
        policy,
        EvalRequest {
            tool,
            args: &args,
            working_dir: ws(),
        },
    )
}

#[test]
fn fd_and_rg_tools_are_baseline_allowed_with_path_checks() {
    let policy = build_policy(None, None);
    assert_eq!(check_eval(&policy, "fd", json!({"pattern": "main"})), Decision::Allow);
    assert_eq!(
        check_eval(&policy, "rg", json!({"pattern": "todo", "path": "src"})),
        Decision::Allow
    );
    assert_eq!(
        check_eval(&policy, "rg", json!({"pattern": "x", "path": "/etc"})),
        Decision::Ask
    );

    let scope =
        parse_scope_from_str("[permission.path]\n\"*.env*\" = { action = \"deny\", reason = \"secrets\" }\n").unwrap();
    let policy = build_policy(Some(scope), None);
    assert_eq!(
        check_eval(&policy, "rg", json!({"pattern": "key", "path": ".env"})),
        Decision::Deny("secrets".to_string())
    );
}

#[test]
fn paths_outside_working_dir_always_ask() {
    let scope = parse_scope_from_str("[allow]\nread = [\"*\"]\nwrite = [\"*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);

    for path in ["/etc/passwd", "../sibling/x", "a/../../etc/x"] {
        assert_eq!(check_eval(&policy, "read", json!({"path": path})), Decision::Ask);
    }
    assert_eq!(
        check_eval(&policy, "read", json!({"path": "src/../main.rs"})),
        Decision::Allow
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "src/main.rs"}),
                working_dir: None
            }
        ),
        Decision::Ask
    );
}

#[test]
fn paths_denied_by_permission_rule() {
    let scope = parse_scope_from_str("[deny]\nread = [\"/tmp/*\", \"*.env*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    assert_eq!(
        check_eval(&policy, "read", json!({"path": "/tmp/file.txt"})),
        Decision::Deny("denied by permission rule 'read|/tmp/*'".to_string())
    );
    assert_eq!(
        check_eval(&policy, "read", json!({"path": "secrets.env"})),
        Decision::Deny("denied by permission rule 'read|*.env*'".to_string())
    );
}

#[test]
fn match_input_and_tool_aliases() {
    let tool_cases = [
        ("webfetch", "fetch"),
        ("web_fetch", "fetch"),
        ("websearch", "search"),
        ("web_search", "search"),
        ("bash", "bash"),
    ];
    for (alias, canonical) in tool_cases {
        assert_eq!(canonical_tool(alias), canonical);
    }

    let input_cases = [
        (json!({"command": "cargo test"}), "cargo test"),
        (json!({"path": "/tmp/x"}), "/tmp/x"),
        (json!({"url": "https://x.com"}), "https://x.com"),
        (json!({"query": "rust"}), "rust"),
        (json!({"arg": 1, "tool": "mcp"}), r#"{"arg":1,"tool":"mcp"}"#),
    ];
    for (arg, expected) in input_cases {
        assert_eq!(match_input(&arg), expected);
    }
}

#[test]
fn suggested_rules_semantics() {
    let cases = [
        ("bash", "cargo test --nocapture", "cargo test *"),
        ("bash", "cat /etc/hosts", "cat *"),
        ("bash", "ls", "ls *"),
        ("read", "src/main.rs", "src/main.rs/*"),
        ("write", "/tmp/notes.txt", "/tmp/notes.txt/*"),
        ("fetch", "https://github.com/x/y?z=1", "https://github.com/*"),
        ("fetch", "not a url", "*"),
        ("search", "rust async", "*"),
    ];
    for (tool, input, expected) in cases {
        assert_eq!(suggested_rule(tool, input), expected);
    }
}

#[test]
fn ask_drafts_targets_single_and_empty() {
    let policy = build_policy(None, None);
    let drafts = ask_drafts(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "cat /etc/hosts"}),
            working_dir: ws(),
        },
    );
    assert_eq!(
        (drafts.len(), drafts[0].surface.as_str(), drafts[0].pattern.as_str()),
        (1, "path", "/etc/hosts/*")
    );

    let drafts = ask_drafts(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "cat src/main.rs"}),
            working_dir: ws(),
        },
    );
    assert!(drafts.is_empty());
}

#[test]
fn ask_drafts_targets_multiple() {
    let policy = build_policy(None, None);
    let drafts = ask_drafts(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "python3 /tmp/gen.py"}),
            working_dir: ws(),
        },
    );
    assert_eq!(drafts.len(), 2);
    assert_eq!(
        (drafts[0].surface.as_str(), drafts[0].pattern.as_str()),
        ("bash", "python3 /tmp/gen.py")
    );
    assert_eq!(
        (drafts[1].surface.as_str(), drafts[1].pattern.as_str()),
        ("path", "/tmp/gen.py/*")
    );
}

#[test]
fn saved_rules_round_trip_with_comments() {
    let dir = temp_dir("round_trip");
    let path = dir.join("permission.toml");
    std::fs::write(&path, "# my rules\n[permission.bash]\n\"git *\" = \"allow\" # safe\n").unwrap();

    save_allow_rule(&path, "bash", "cargo test *").unwrap();
    save_allow_rule(&path, "bash", "cargo test *").unwrap();

    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# my rules"), "comment lost:\n{saved}");
    assert!(saved.contains("# safe"), "comment lost:\n{saved}");
    assert_eq!(saved.matches("cargo test *").count(), 1, "duplicate rule:\n{saved}");

    let parsed_scope = parse_scope_from_str(&saved).unwrap();
    assert!(
        parsed_scope
            .rules
            .iter()
            .any(|r| r.surface == "bash" && r.pattern == "cargo test *")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn path_tool_always_allow_saves_to_path_surface_and_overrides_workspace_ask() {
    let dir = temp_dir("path_surface_save");
    let path = dir.join("permission.toml");
    save_allow_rule(&path, "path", "/etc/hosts/*").unwrap();

    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("[permission.path]"), "surface table missing:\n{saved}");

    let scope = parse_scope_from_str(&saved).unwrap();
    let policy = build_policy(Some(scope), None);

    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "/etc/hosts"}),
                working_dir: ws()
            }
        ),
        Decision::Allow
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_never_clobbers_a_malformed_file() {
    let dir = temp_dir("malformed");
    let path = dir.join("permission.toml");
    std::fs::write(&path, "[allow\n").unwrap();
    assert!(save_allow_rule(&path, "bash", "cargo test *").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[allow\n");
    let _ = std::fs::remove_dir_all(dir);
}

fn check_default_bash(cmd: &str) -> Decision {
    let policy = build_policy(None, None);
    decide_tool_call(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": cmd}),
            working_dir: ws(),
        },
    )
}

#[test]
fn default_policy_allows_safe_bash() {
    for cmd in [
        "git status",
        "git branch --show-current",
        "cargo check",
        "cargo test",
        "ls -la",
        "pwd",
    ] {
        assert_eq!(check_default_bash(cmd), Decision::Allow);
    }
}

#[test]
fn default_policy_asks_for_unknown_bash() {
    for cmd in [
        "make clippy",
        "git commit -m 'feat: test'",
        "curl https://example.com",
        "npm install",
    ] {
        assert_eq!(check_default_bash(cmd), Decision::Ask);
    }
}

#[test]
fn mcp_direct_tool_permission_evaluation() {
    let scope = crate::permission::policy::parse_scope_from_str(
        "[allow]\nmcp = [\"github:*\"]\n[deny]\nmcp = [\"postgres:drop_db\"]\n",
    )
    .unwrap();
    let policy = crate::permission::policy::build_policy(Some(scope), None);

    let allow_req = EvalRequest {
        tool: "github_create_issue",
        args: &json!({"title": "Bug"}),
        working_dir: None,
    };
    assert_eq!(decide_tool_call(&policy, allow_req), Decision::Allow);

    let deny_req = EvalRequest {
        tool: "postgres_drop_db",
        args: &json!({}),
        working_dir: None,
    };
    assert!(matches!(decide_tool_call(&policy, deny_req), Decision::Deny(_)));
}
