use crate::permission::eval::ask_drafts;
use crate::permission::policy::{build_policy, parse_scope_from_str, save_allow_rule};
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

#[test]
fn fd_and_rg_tools_are_baseline_allowed_with_path_checks() {
    let policy = build_policy(None, None);
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "fd",
                args: &json!({"pattern": "main"}),
                working_dir: ws()
            }
        ),
        Decision::Allow
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "rg",
                args: &json!({"pattern": "todo", "path": "src"}),
                working_dir: ws()
            }
        ),
        Decision::Allow
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "rg",
                args: &json!({"pattern": "x", "path": "/etc"}),
                working_dir: ws()
            }
        ),
        Decision::Ask
    );

    let scope =
        parse_scope_from_str("[permission.path]\n\"*.env*\" = { action = \"deny\", reason = \"secrets\" }\n").unwrap();
    let policy = build_policy(Some(scope), None);
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "rg",
                args: &json!({"pattern": "key", "path": ".env"}),
                working_dir: ws()
            }
        ),
        Decision::Deny("secrets".to_string())
    );
}

#[test]
fn paths_outside_working_dir_always_ask() {
    let scope = parse_scope_from_str("[allow]\nread = [\"*\"]\nwrite = [\"*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);

    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "/etc/passwd"}),
                working_dir: ws()
            }
        ),
        Decision::Ask
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "../sibling/x"}),
                working_dir: ws()
            }
        ),
        Decision::Ask
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "a/../../etc/x"}),
                working_dir: ws()
            }
        ),
        Decision::Ask
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "src/../main.rs"}),
                working_dir: ws()
            }
        ),
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

    let scope = parse_scope_from_str("[deny]\nread = [\"/tmp/*\", \"*.env*\"]\n").unwrap();
    let policy = build_policy(Some(scope), None);
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "/tmp/file.txt"}),
                working_dir: ws()
            }
        ),
        Decision::Deny("denied by permission rule 'read|/tmp/*'".to_string())
    );
    assert_eq!(
        decide_tool_call(
            &policy,
            EvalRequest {
                tool: "read",
                args: &json!({"path": "secrets.env"}),
                working_dir: ws()
            }
        ),
        Decision::Deny("denied by permission rule 'read|*.env*'".to_string())
    );
}

#[test]
fn match_input_and_tool_aliases() {
    assert_eq!(canonical_tool("webfetch"), "fetch");
    assert_eq!(canonical_tool("web_fetch"), "fetch");
    assert_eq!(canonical_tool("websearch"), "search");
    assert_eq!(canonical_tool("web_search"), "search");
    assert_eq!(canonical_tool("bash"), "bash");

    assert_eq!(match_input(&json!({"command": "cargo test"})), "cargo test");
    assert_eq!(match_input(&json!({"path": "/tmp/x"})), "/tmp/x");
    assert_eq!(match_input(&json!({"url": "https://x.com"})), "https://x.com");
    assert_eq!(match_input(&json!({"query": "rust"})), "rust");
    assert_eq!(
        match_input(&json!({"tool": "mcp", "arg": 1})),
        r#"{"arg":1,"tool":"mcp"}"#
    );
}

#[test]
fn suggested_rules_semantics() {
    assert_eq!(suggested_rule("bash", "cargo test --nocapture"), "cargo test *");
    assert_eq!(suggested_rule("bash", "cat /etc/hosts"), "cat *");
    assert_eq!(suggested_rule("bash", "ls"), "ls *");
    assert_eq!(suggested_rule("read", "src/main.rs"), "src/main.rs/*");
    assert_eq!(suggested_rule("write", "/tmp/notes.txt"), "/tmp/notes.txt/*");
    assert_eq!(
        suggested_rule("fetch", "https://github.com/x/y?z=1"),
        "https://github.com/*"
    );
    assert_eq!(suggested_rule("fetch", "not a url"), "*");
    assert_eq!(suggested_rule("search", "rust async"), "*");
}

#[test]
fn ask_drafts_target_the_components_that_asked() {
    let policy = build_policy(None, None);
    let drafts = ask_drafts(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "cat /etc/hosts"}),
            working_dir: ws(),
        },
    );
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].surface, "path");
    assert_eq!(drafts[0].pattern, "/etc/hosts/*");

    let drafts = ask_drafts(
        &policy,
        EvalRequest {
            tool: "bash",
            args: &json!({"command": "python3 /tmp/gen.py"}),
            working_dir: ws(),
        },
    );
    assert_eq!(drafts.len(), 2);
    assert_eq!(drafts[0].surface, "bash");
    assert_eq!(drafts[0].pattern, "python3 /tmp/gen.py");
    assert_eq!(drafts[1].surface, "path");
    assert_eq!(drafts[1].pattern, "/tmp/gen.py/*");

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

#[test]
fn default_policy_allows_safe_bash_and_asks_for_unknown() {
    let policy = build_policy(None, None);
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
    assert_eq!(check("git branch --show-current"), Decision::Allow);
    assert_eq!(check("cargo check"), Decision::Allow);
    assert_eq!(check("cargo test"), Decision::Allow);
    assert_eq!(check("ls -la"), Decision::Allow);
    assert_eq!(check("pwd"), Decision::Allow);

    assert_eq!(check("make clippy"), Decision::Ask);
    assert_eq!(check("git commit -m 'feat: test'"), Decision::Ask);
    assert_eq!(check("curl https://example.com"), Decision::Ask);
    assert_eq!(check("npm install"), Decision::Ask);
}
