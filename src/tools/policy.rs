use crate::tools::ask_user::AskUserArgs;
use crate::tools::bash::BashArgs;
use crate::tools::bash_ast::{RiskTier, analyze_command_safety};
use crate::tools::edit::EditArgs;
use crate::tools::read::ReadArgs;
use crate::tools::web::fetch::FetchArgs;
use crate::tools::web::search::SearchArgs;
use crate::tools::write::WriteArgs;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionClass {
    ReadOnly,
    WorkspaceMutation,
    ApprovalRequired { tier: RiskTier, reasons: Vec<String> },
}

impl ExecutionClass {
    pub fn allows_without_approval(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::WorkspaceMutation)
    }

    fn mutating(reason: impl Into<String>) -> Self {
        Self::ApprovalRequired {
            tier: RiskTier::Mutating,
            reasons: vec![reason.into()],
        }
    }
}

pub struct ToolExecutionPolicy;

impl ToolExecutionPolicy {
    pub fn classify(tool_name: &str, arguments: &Value) -> ExecutionClass {
        let working_dir = std::env::current_dir().unwrap_or_default();
        Self::classify_in(tool_name, arguments, &working_dir)
    }

    fn classify_in(tool_name: &str, arguments: &Value, working_dir: &Path) -> ExecutionClass {
        match tool_name {
            "read" if valid::<ReadArgs>(arguments) => ExecutionClass::ReadOnly,
            "websearch" if valid::<SearchArgs>(arguments) => ExecutionClass::ReadOnly,
            "webfetch" if valid::<FetchArgs>(arguments) => ExecutionClass::ReadOnly,
            "ask_user" | "ask_user_question" => ExecutionClass::ReadOnly,
            "write" => classify_write(arguments, working_dir),
            "edit" => classify_edit(arguments, working_dir),
            "bash" => classify_bash(arguments),
            _ => ExecutionClass::mutating("Unknown or malformed tool call cannot be proven read-only"),
        }
    }

    pub fn canonical_arguments(tool_name: &str, arguments: &Value) -> Option<Value> {
        match tool_name {
            "read" => canonical::<ReadArgs>(arguments),
            "write" => canonical::<WriteArgs>(arguments),
            "edit" => canonical::<EditArgs>(arguments),
            "bash" => canonical::<BashArgs>(arguments),
            "websearch" => canonical::<SearchArgs>(arguments),
            "webfetch" => canonical::<FetchArgs>(arguments),
            "ask_user" | "ask_user_question" => canonical::<AskUserArgs>(arguments),
            _ => None,
        }
    }
}

fn canonical<T>(arguments: &Value) -> Option<Value>
where
    T: DeserializeOwned + serde::Serialize,
{
    serde_json::from_value::<T>(arguments.clone())
        .ok()
        .and_then(|arguments| serde_json::to_value(arguments).ok())
}

fn valid<T: DeserializeOwned>(arguments: &Value) -> bool {
    serde_json::from_value::<T>(arguments.clone()).is_ok()
}

fn classify_write(arguments: &Value, working_dir: &Path) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<WriteArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed write arguments cannot be validated safely");
    };
    classify_file_path(&args.path, working_dir, "Write target exits the working directory")
}

fn classify_edit(arguments: &Value, working_dir: &Path) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<EditArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed edit arguments cannot be validated safely");
    };
    classify_file_path(&args.path, working_dir, "Edit target exits the working directory")
}

fn classify_file_path(path: &str, working_dir: &Path, outside_reason: &str) -> ExecutionClass {
    if is_protected_workspace_path(path, working_dir) {
        return ExecutionClass::mutating("Target is protected repository metadata: .git");
    }
    if path_is_within_working_dir(path, working_dir) {
        ExecutionClass::WorkspaceMutation
    } else {
        ExecutionClass::mutating(outside_reason)
    }
}

fn path_is_within_working_dir(raw_path: &str, working_dir: &Path) -> bool {
    let clean_path = raw_path.trim().trim_matches(['\'', '"']);
    if clean_path.is_empty() {
        return false;
    }
    let Ok(root) = working_dir.canonicalize() else {
        return false;
    };
    let path = Path::new(clean_path);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    canonical_existing_ancestor(&candidate).is_some_and(|ancestor| ancestor.starts_with(&root))
}

fn is_protected_workspace_path(raw_path: &str, working_dir: &Path) -> bool {
    let clean_path = raw_path.trim().trim_matches(['\'', '"']);
    let path = Path::new(clean_path);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    };
    candidate
        .strip_prefix(working_dir)
        .ok()
        .is_some_and(|relative| relative.components().any(|component| component.as_os_str() == ".git"))
}

fn canonical_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut ancestor = path;
    loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            return Some(canonical);
        }
        ancestor = ancestor.parent()?;
    }
}

fn classify_bash(arguments: &Value) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<BashArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed bash arguments cannot be analyzed safely");
    };
    let analysis = analyze_command_safety(&args.command);
    if analysis.tier == RiskTier::ReadOnly {
        ExecutionClass::ReadOnly
    } else {
        ExecutionClass::ApprovalRequired {
            tier: analysis.tier,
            reasons: analysis.reasons,
        }
    }
}

#[cfg(test)]
mod tests {
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

        let class =
            ToolExecutionPolicy::classify_in("write", &json!({"path": "escape/out.txt", "content": "x"}), &root);

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
}
