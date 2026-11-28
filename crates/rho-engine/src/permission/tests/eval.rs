use crate::permission::eval::ask_drafts;
use crate::permission::policy::{Policy, build_policy, parse_scope_from_str, save_allow_rule};
use crate::permission::suggest::{canonical_tool, match_input, suggested_rule};
use crate::permission::{Decision, EvalRequest, decide_tool_call};
use crate::test_support::temp_dir;
use rho_test_support::EnvVarGuard;
use serde_json::json;
use std::path::Path;
use test_case::test_case;

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

#[test_case("webfetch", "fetch"; "webfetch")]
#[test_case("web_fetch", "fetch"; "web fetch")]
#[test_case("websearch", "search"; "websearch")]
#[test_case("web_search", "search"; "web search")]
#[test_case("bash", "bash"; "bash")]
fn tool_aliases_resolve_to_canonical_names(alias: &str, canonical: &str) {
    assert_eq!(canonical_tool(alias), canonical);
}

#[test_case(json!({"command": "cargo test"}), "cargo test"; "command")]
#[test_case(json!({"path": "/tmp/x"}), "/tmp/x"; "path")]
#[test_case(json!({"url": "https://x.com"}), "https://x.com"; "url")]
#[test_case(json!({"query": "rust"}), "rust"; "query")]
#[test_case(json!({"arg": 1, "tool": "mcp"}), r#"{"arg":1,"tool":"mcp"}"#; "other args as json")]
fn match_input_uses_the_primary_argument(arg: serde_json::Value, expected: &str) {
    assert_eq!(match_input(&arg), expected);
}

#[test_case("bash", "cargo test --nocapture", "cargo test *"; "bash two word prefix")]
#[test_case("bash", "cat /etc/hosts", "cat *"; "bash one word prefix")]
#[test_case("bash", "ls", "ls *"; "bash bare command")]
#[test_case("read", "src/main.rs", "src/main.rs/*"; "read path")]
#[test_case("write", "/tmp/notes.txt", "/tmp/notes.txt/*"; "write path")]
#[test_case("undo", "src/main.rs", "src/main.rs/*"; "undo path")]
#[test_case("lsp", "src/main.rs", "src/main.rs/*"; "lsp path")]
#[test_case("lsp", "{\"action\":\"status\"}", "*"; "lsp without path")]
#[test_case("fetch", "https://github.com/x/y?z=1", "https://github.com/*"; "fetch url origin")]
#[test_case("fetch", "not a url", "*"; "fetch invalid url")]
#[test_case("search", "rust async", "*"; "search")]
fn suggested_rules_semantics(tool: &str, input: &str, expected: &str) {
    assert_eq!(suggested_rule(tool, input), expected);
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
}

#[cfg(unix)]
#[test]
fn saving_never_overwrites_an_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("unreadable_save");
    let path = dir.join("permission.toml");
    let original = "[permission.bash]\n\"git *\" = \"allow\"\n";
    std::fs::write(&path, original).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    assert!(save_allow_rule(&path, "bash", "cargo test *").is_err());

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn saving_keeps_the_existing_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("mode_save");
    let path = dir.join("permission.toml");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    save_allow_rule(&path, "bash", "cargo test *").unwrap();

    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn saving_never_clobbers_a_malformed_file() {
    let dir = temp_dir("malformed");
    let path = dir.join("permission.toml");
    std::fs::write(&path, "[allow\n").unwrap();
    assert!(save_allow_rule(&path, "bash", "cargo test *").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[allow\n");
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
fn memory_notes_need_no_approval_unless_a_rule_says_otherwise() {
    let note = json!({"action": "add", "text": "run the fidelity tests first"});
    assert_eq!(
        check_eval(&build_policy(None, None), "memory", note.clone()),
        Decision::Allow
    );

    let scope = parse_scope_from_str("[deny]\nmemory = [\"*\"]\n").unwrap();
    assert!(matches!(
        check_eval(&build_policy(Some(scope), None), "memory", note),
        Decision::Deny(_)
    ));
}

#[test_case("git status"; "git status")]
#[test_case("cargo check"; "cargo check")]
#[test_case("cargo test"; "cargo test")]
#[test_case("ls -la"; "ls")]
#[test_case("pwd"; "pwd")]
#[test_case("make clippy"; "make clippy")]
#[test_case("git commit -m 'feat: test'"; "git commit")]
#[test_case("curl https://example.com"; "curl")]
#[test_case("npm install"; "npm install")]
fn default_policy_asks_for_unconfigured_bash(cmd: &str) {
    assert_eq!(check_default_bash(cmd), Decision::Ask);
}

#[test_case("git status"; "git status")]
#[test_case("git diff"; "git diff")]
#[test_case("cargo check"; "cargo check")]
#[test_case("cargo test"; "cargo test")]
fn policy_allows_explicit_bash_rules(cmd: &str) {
    let scope = parse_scope_from_str("[permission.bash]\n\"git *\" = \"allow\"\n\"cargo *\" = \"allow\"\n").unwrap();
    let policy = build_policy(Some(scope), None);

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
fn mcp_gateway_calls_follow_the_same_server_tool_rules() {
    let scope = crate::permission::policy::parse_scope_from_str(
        "[allow]\nmcp = [\"github:*\"]\n[deny]\nmcp = [\"postgres:drop_db\"]\n",
    )
    .unwrap();
    let policy = crate::permission::policy::build_policy(Some(scope), None);
    let call = |server: &str, tool: &str| {
        decide_with_servers(
            &policy,
            "mcp",
            &json!({"server": server, "tool": tool, "arguments": {}}),
            &[],
        )
    };
    assert_eq!(call("github", "create_issue"), Decision::Allow);
    assert!(matches!(call("postgres", "drop_db"), Decision::Deny(_)));
    assert_eq!(call("postgres", "list_tables"), Decision::Ask);
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

#[test_case("read"; "read")]
#[test_case("write"; "write")]
#[test_case("edit"; "edit")]
fn credential_store_is_denied_even_with_allow_rule(tool: &'static str) {
    let (config_dir, _env) = super::config_home();
    let auth = config_dir.join("auth.json").display().to_string();
    let policy = build_policy(Some(parse_scope_from_str("[allow]\npath = [\"*\"]\n").unwrap()), None);
    let dec = check_eval(&policy, tool, json!({"path": auth}));
    assert!(matches!(dec, Decision::Deny(_)), "{dec:?}");
}

#[test_case("/ws/.config/rho/permission.toml"; "rho config policy")]
#[test_case("/ws/.GIT/config"; "uppercase git")]
#[test_case("/ws/.Rho/permission.toml"; "mixed case rho")]
fn protected_names_are_case_insensitive_and_policy_files_need_approval(path: &str) {
    let policy = build_policy(None, None);
    assert_eq!(check_eval(&policy, "write", json!({"path": path})), Decision::Ask);
}

#[test]
fn prompts_read_without_approval_but_config_files_do_not() {
    let (config_dir, _env) = super::config_home();
    let policy = build_policy(None, None);
    let prompt = config_dir.join("prompts/review.md").display().to_string();
    assert_eq!(check_eval(&policy, "read", json!({"path": prompt})), Decision::Allow);
    let permission = config_dir.join("permission.toml").display().to_string();
    assert_eq!(check_eval(&policy, "read", json!({"path": permission})), Decision::Ask);
}

#[test]
fn bash_deny_rules_apply_to_obfuscated_and_wrapped_commands() {
    let scope = parse_scope_from_str("[deny]\nbash = [\"rm -rf *\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let denied = [
        "rm -rf build",
        "\"rm\" -rf build",
        "r''m -rf build",
        "\\rm -rf build",
        "/bin/rm -rf build",
        "env rm -rf build",
        "sudo rm -rf build",
        "sudo -u root rm -rf build",
        "xargs -0 rm -rf",
        "{ rm -rf build; }",
        "if true; then rm -rf build; fi",
        "echo hi && /usr/bin/env rm -rf build",
    ];
    for command in denied {
        assert!(
            matches!(
                check_eval(&policy, "bash", json!({"command": command})),
                Decision::Deny(_)
            ),
            "{command}"
        );
    }
    assert_eq!(
        check_eval(&policy, "bash", json!({"command": "ls build"})),
        Decision::Ask
    );
}

#[test]
fn bash_reads_of_the_credential_file_through_env_variables_are_denied() {
    let dir = crate::test_support::TestDir::new();
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    let _env = EnvVarGuard::set("RHO_HOME", &dir.to_string_lossy());
    let policy = build_policy(None, None);
    let decisions = [
        "cat $RHO_HOME/auth.json",
        "cat \"$RHO_HOME\"/auth.json",
        "cat ${RHO_HOME}/auth.json",
    ]
    .map(|command| check_eval(&policy, "bash", json!({"command": command})));
    for decision in decisions {
        assert!(matches!(decision, Decision::Deny(_)));
    }
}
