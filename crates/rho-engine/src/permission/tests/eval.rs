use crate::permission::eval::ask_drafts;
use crate::permission::policy::{Policy, build_policy, parse_scope_from_str, save_allow_rule};
use crate::permission::suggest::{canonical_tool, match_input, suggested_rule};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use crate::test_support::temp_dir;
use serde_json::json;
use std::path::Path;

fn ws() -> Option<&'static Path> {
    Some(Path::new("/ws"))
}

fn canonical_glob(dir: &str) -> String {
    format!(
        "{}/*",
        rho_harness_core::workspace::canonicalize_path(Path::new(dir)).display()
    )
}

fn check_eval(policy: &Policy, tool: &'static str, args: serde_json::Value) -> Decision {
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

fn assert_read_decisions(scope_toml: &str, cases: Vec<(&str, Decision)>) {
    let policy = build_policy(Some(parse_scope_from_str(scope_toml).unwrap()), None);
    for (path, expected) in cases {
        assert_eq!(check_eval(&policy, "read", json!({"path": path})), expected, "{path}");
    }
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
        assert_eq!(
            check_eval(&policy, "write", json!({"path": path, "content": "x"})),
            Decision::Ask
        );
        assert_eq!(
            check_eval(&policy, "edit", json!({"path": path, "edits": []})),
            Decision::Ask
        );
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
                mcp_servers: &[],
                working_dir: None
            }
        ),
        Decision::Ask
    );
}

#[test]
fn protected_paths_always_ask() {
    let policy = build_policy(None, None);
    let protected = [
        ".git/config",
        ".git/HEAD",
        ".rho/config.toml",
        ".env",
        ".env.local",
        "sub/.env.production",
    ];
    for path in protected {
        assert_eq!(
            check_eval(&policy, "read", json!({"path": path})),
            Decision::Ask,
            "expected {path} to ask"
        );
        assert_eq!(
            check_eval(&policy, "write", json!({"path": path, "content": ""})),
            Decision::Ask,
            "expected {path} to ask"
        );
    }
    assert_eq!(
        check_eval(&policy, "read", json!({"path": "src/main.rs"})),
        Decision::Allow
    );

    let scope = parse_scope_from_str("[permission.path]\n\".env\" = \"allow\"\n").unwrap();
    let allow_policy = build_policy(Some(scope), None);
    assert_eq!(
        check_eval(&allow_policy, "read", json!({"path": ".env"})),
        Decision::Allow
    );
}

#[test]
fn paths_denied_by_permission_rule() {
    assert_read_decisions(
        "[deny]\nread = [\"/tmp/*\", \"*.env*\"]\n",
        vec![
            (
                "/tmp/file.txt",
                Decision::Deny("denied by permission rule 'read|/tmp/*'".to_string()),
            ),
            (
                "secrets.env",
                Decision::Deny("denied by permission rule 'read|*.env*'".to_string()),
            ),
        ],
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
            mcp_servers: &[],
            working_dir: ws(),
        },
    );
    assert_eq!(drafts.len(), 2);
    assert_eq!(
        (drafts[0].surface.as_str(), drafts[0].pattern.as_str()),
        ("bash", "cat /etc/hosts")
    );
    assert_eq!(
        (drafts[1].surface.as_str(), drafts[1].pattern.as_str()),
        ("path", canonical_glob("/etc/hosts").as_str())
    );

    let scope = parse_scope_from_str("[permission.bash]\n\"cat *\" = \"allow\"\n").unwrap();
    let policy_with_allow = build_policy(Some(scope), None);
    let drafts = ask_drafts(
        &policy_with_allow,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "cat src/main.rs"}),
            mcp_servers: &[],
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
            mcp_servers: &[],
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
        ("path", canonical_glob("/tmp/gen.py").as_str())
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
        check_eval(&policy, "read", json!({"path": "/etc/hosts"})),
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
            mcp_servers: &[],
            working_dir: ws(),
        },
    )
}

#[test]
fn default_policy_asks_for_unconfigured_bash() {
    for cmd in [
        "git status",
        "cargo check",
        "cargo test",
        "ls -la",
        "pwd",
        "make clippy",
        "git commit -m 'feat: test'",
        "curl https://example.com",
        "npm install",
    ] {
        assert_eq!(check_default_bash(cmd), Decision::Ask);
    }
}

#[test]
fn policy_allows_explicit_bash_rules() {
    let scope = parse_scope_from_str("[permission.bash]\n\"git *\" = \"allow\"\n\"cargo *\" = \"allow\"\n").unwrap();
    let policy = build_policy(Some(scope), None);

    for cmd in ["git status", "git diff", "cargo check", "cargo test"] {
        let dec = decide_tool_call(
            &policy,
            EvalRequest {
                tool: "bash",
                args: &json!({"command": cmd}),
                mcp_servers: &[],
                working_dir: ws(),
            },
        );
        assert_eq!(dec, Decision::Allow);
    }
}

#[test]
fn mcp_direct_tool_permission_evaluation() {
    let scope = crate::permission::policy::parse_scope_from_str(
        "[allow]\nmcp = [\"github:*\"]\n[deny]\nmcp = [\"postgres:drop_db\"]\n",
    )
    .unwrap();
    let policy = crate::permission::policy::build_policy(Some(scope), None);

    let servers = ["github".to_string(), "postgres".to_string()];
    let allow_req = EvalRequest {
        tool: "github_create_issue",
        args: &json!({"title": "Bug"}),
        mcp_servers: &servers,
        working_dir: None,
    };
    assert_eq!(decide_tool_call(&policy, allow_req), Decision::Allow);

    let deny_req = EvalRequest {
        tool: "postgres_drop_db",
        args: &json!({}),
        mcp_servers: &servers,
        working_dir: None,
    };
    assert!(matches!(decide_tool_call(&policy, deny_req), Decision::Deny(_)));
}

#[test]
fn test_parse_universal_permission_action() {
    let scope_allow = parse_scope_from_str("[permission]\n\"*\" = \"allow\"\n").unwrap();
    assert_eq!(scope_allow.universal, Some(crate::permission::PermissionState::Allow));
    let scope_deny = parse_scope_from_str("[permission]\n\"*\" = \"deny\"\n").unwrap();
    assert_eq!(scope_deny.universal, Some(crate::permission::PermissionState::Deny));
    let scope_obj = parse_scope_from_str("[permission]\n\"*\" = { action = \"deny\" }\n").unwrap();
    assert_eq!(scope_obj.universal, None);
}

fn decide_with_servers(policy: &Policy, tool: &str, args: &serde_json::Value, servers: &[String]) -> Decision {
    decide_tool_call(
        policy,
        EvalRequest {
            tool,
            args,
            working_dir: ws(),
            mcp_servers: servers,
        },
    )
}

#[test]
fn mcp_wire_names_split_on_configured_server_prefix() {
    let scope = parse_scope_from_str("[allow]\nmcp = [\"my_server:list_items\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let servers = ["my_server".to_string()];
    assert_eq!(
        decide_with_servers(&policy, "my_server_list_items", &json!({}), &servers),
        Decision::Allow
    );
}

#[test]
fn builtin_web_fetch_is_not_split_into_mcp_server_web() {
    let scope = parse_scope_from_str("[allow]\nmcp = [\"web:*\"]\n[deny]\nweb_fetch = [\"*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let dec = decide_with_servers(&policy, "web_fetch", &json!({"url": "https://example.com"}), &[]);
    assert!(matches!(dec, Decision::Deny(_)), "{dec:?}");
}

#[test]
fn path_rules_match_only_canonical_and_cwd_relative_values() {
    assert_read_decisions(
        "[allow]\npath = [\"src/*\"]\n",
        vec![
            ("src/main.rs", Decision::Allow),
            ("src/../../etc/passwd", Decision::Ask),
            ("src/../.git/config", Decision::Ask),
        ],
    );
}

#[test]
fn quoted_absolute_path_is_gated_as_outside_workspace() {
    let policy = build_policy(None, None);
    assert_eq!(
        check_eval(&policy, "write", json!({"path": "\"/etc/authorized_keys\""})),
        Decision::Ask
    );
}

#[test]
fn credential_store_is_denied_even_with_allow_rule() {
    let _env = super::ENV_LOCK.blocking_lock();
    let Some(config_dir) = crate::permission::policy::config_dir() else {
        return;
    };
    let auth = config_dir.join("auth.json").display().to_string();
    let policy = build_policy(Some(parse_scope_from_str("[allow]\npath = [\"*\"]\n").unwrap()), None);
    for tool in ["read", "write", "edit"] {
        let dec = check_eval(&policy, tool, json!({"path": auth}));
        assert!(matches!(dec, Decision::Deny(_)), "{tool}: {dec:?}");
    }
}

#[test]
fn protected_names_are_case_insensitive_and_policy_files_need_approval() {
    let policy = build_policy(None, None);
    for path in [
        "/ws/.config/rho/permission.toml",
        "/ws/.GIT/config",
        "/ws/.Rho/permission.toml",
    ] {
        assert_eq!(
            check_eval(&policy, "write", json!({"path": path})),
            Decision::Ask,
            "{path}"
        );
    }
}

#[test]
fn prompts_read_without_approval_but_config_files_do_not() {
    let _env = super::ENV_LOCK.blocking_lock();
    let Some(config_dir) = crate::permission::policy::config_dir() else {
        return;
    };
    let policy = build_policy(None, None);
    let prompt = config_dir.join("prompts/review.md").display().to_string();
    assert_eq!(check_eval(&policy, "read", json!({"path": prompt})), Decision::Allow);
    let permission = config_dir.join("permission.toml").display().to_string();
    assert_eq!(check_eval(&policy, "read", json!({"path": permission})), Decision::Ask);
}
