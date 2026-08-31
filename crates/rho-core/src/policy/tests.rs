use super::*;
use serde_json::json;

#[test]
fn classifies_every_known_tool() {
    for (name, arguments, read_only) in [
        ("read", json!({"path": "src/lib.rs"}), true),
        ("write", json!({"path": "out.txt", "content": "x"}), true),
        (
            "edit",
            json!({"path": "out.txt", "edits": [{"oldText": "x", "newText": "y"}]}),
            true,
        ),
        ("bash", json!({"command": "git status"}), true),
        ("websearch", json!({"query": "rust"}), true),
        ("webfetch", json!({"url": "https://example.com"}), true),
        ("ask_user", json!({"question": "which option?"}), true),
        (
            "ask_user",
            json!({"question": "which option?", "options": [{"label": "A", "description": "desc"}]}),
            true,
        ),
        (
            "ask_user_question",
            json!({"questions": [{"question": "which option?"}]}),
            true,
        ),
        ("ask_user_question", json!({"question": "which option?"}), true),
        (
            "agent",
            json!({"subagent_type": "explore", "prompt": "find code"}),
            true,
        ),
        (
            "Agent",
            json!({"subagent_type": "explore", "prompt": "find code"}),
            true,
        ),
        ("get_subagent_result", json!({"agent_id": "job_123"}), true),
        (
            "steer_subagent",
            json!({"agent_id": "job_123", "message": "focus on x"}),
            true,
        ),
        ("todo", json!({"action": "list"}), true),
        ("todo", json!({"action": "create", "subject": "test"}), true),
    ] {
        assert_eq!(
            ToolExecutionPolicy::classify(name, &arguments).allows_without_approval(),
            read_only,
            "{name}"
        );
    }
}

#[test]
fn malformed_and_unknown_calls_require_approval() {
    for (name, arguments) in [
        ("read", json!({})),
        ("websearch", json!({"query": 1})),
        ("webfetch", json!(null)),
        ("write", json!({"path": "out.txt"})),
        ("edit", json!({"path": "out.txt"})),
        ("bash", json!({"command": ["ls"]})),
        ("unknown", json!({})),
    ] {
        assert!(
            matches!(
                ToolExecutionPolicy::classify(name, &arguments),
                ExecutionClass::ApprovalRequired { .. }
            ),
            "{name} {arguments}"
        );
    }
}

#[test]
fn external_tool_arguments_have_a_stable_approval_key() {
    let arguments = json!({"value": 1});
    assert_eq!(
        ToolExecutionPolicy::canonical_arguments("external_tool", &arguments),
        Some(arguments)
    );
}

#[test]
fn contained_file_mutations_are_read_only_and_escaping_paths_require_approval() {
    let root = std::env::temp_dir().join(format!("policy_root_{}", uuid::Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("policy_outside_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    for (tool, arguments) in [
        ("write", json!({"path": "nested/out.txt", "content": "x"})),
        (
            "edit",
            json!({"path": root.join("existing.txt"), "edits": [{"oldText": "x", "newText": "y"}]}),
        ),
    ] {
        assert_eq!(
            ToolExecutionPolicy::classify_in(tool, &arguments, &root),
            ExecutionClass::WorkspaceMutation,
            "{tool}"
        );
    }

    for (tool, arguments) in [
        ("write", json!({"path": outside.join("out.txt"), "content": "x"})),
        (
            "edit",
            json!({"path": "../outside.txt", "edits": [{"oldText": "x", "newText": "y"}]}),
        ),
    ] {
        assert!(matches!(
            ToolExecutionPolicy::classify_in(tool, &arguments, &root),
            ExecutionClass::ApprovalRequired {
                tier: RiskTier::Mutating,
                ..
            }
        ));
    }

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn protected_git_metadata_requires_approval() {
    let root = std::env::temp_dir().join(format!("policy_protected_root_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".git")).unwrap();

    for path in [".git/config", "nested/../.git/hooks/pre-commit"] {
        let class = ToolExecutionPolicy::classify_in("write", &json!({"path": path, "content": "x"}), &root);
        assert!(matches!(class, ExecutionClass::ApprovalRequired { .. }), "{path}");
    }

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_escape_requires_approval() {
    let root = std::env::temp_dir().join(format!("policy_symlink_root_{}", uuid::Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("policy_symlink_outside_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();

    let class = ToolExecutionPolicy::classify_in("write", &json!({"path": "escape/out.txt", "content": "x"}), &root);

    assert!(matches!(class, ExecutionClass::ApprovalRequired { .. }));
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn bash_classification_includes_tier_and_reasons() {
    let cases = [
        ("cat Cargo.toml | rg rig", Some(RiskTier::ReadOnly)),
        ("git status && cargo check", Some(RiskTier::ReadOnly)),
        ("cat < Cargo.toml", Some(RiskTier::ReadOnly)),
        ("cat Cargo.toml | tee copy.toml", Some(RiskTier::Mutating)),
        ("cat Cargo.toml > copy.toml", Some(RiskTier::Mutating)),
        ("echo $(touch marker)", Some(RiskTier::Mutating)),
        ("find . -delete", Some(RiskTier::Mutating)),
        ("rm -rf target", Some(RiskTier::HighRisk)),
    ];

    for (command, expected_tier) in cases {
        let class = ToolExecutionPolicy::classify("bash", &json!({"command": command}));
        let tier = match class {
            ExecutionClass::ReadOnly => Some(RiskTier::ReadOnly),
            ExecutionClass::WorkspaceMutation => panic!("bash cannot produce a workspace mutation"),
            ExecutionClass::ApprovalRequired { tier, ref reasons } => {
                assert!(!reasons.is_empty());
                Some(tier)
            }
        };
        assert_eq!(tier, expected_tier, "{command}");
    }
}
