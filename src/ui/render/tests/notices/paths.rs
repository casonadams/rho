use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, clean_command_paths, format_tool_args_summary, read_summary_parts,
    to_relative_path,
};

#[test]
fn bash_summary_formats_timeout_inline() {
    let with_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build", "timeout": 30}));
    assert_eq!(with_timeout, "cargo build (timeout 30s)");

    let without_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build"}));
    assert_eq!(without_timeout, "cargo build");
}

#[test]
fn test_classify_read_path() {
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "/path/to/skills/plan/SKILL.md"})),
        Some(ReadClassification::Skill {
            name: "plan".to_string()
        })
    );
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "AGENTS.md"})),
        Some(ReadClassification::Resource {
            path: "AGENTS.md".to_string()
        })
    );
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "README.md"})),
        Some(ReadClassification::Docs {
            path: "README.md".to_string()
        })
    );
    assert_eq!(classify_read_path(&serde_json::json!({"path": "src/main.rs"})), None);
}

#[test]
fn read_summaries_show_explicit_line_ranges() {
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20})),
        ("src/lib.rs".to_string(), Some(":10-29".to_string()))
    );
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs"})),
        ("src/lib.rs".to_string(), None)
    );
}

#[test]
fn test_to_relative_path() {
    let cwd = std::env::current_dir().unwrap();
    let abs = cwd.join("src/main.rs");
    let rel = to_relative_path(abs.to_str().unwrap());
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_clean_command_paths() {
    let cwd = std::env::current_dir().unwrap();
    let cwd_str = cwd.to_str().unwrap();
    let cmd = format!("cat {cwd_str}/Cargo.toml");
    let cleaned = clean_command_paths(&cmd);
    assert_eq!(cleaned, "cat Cargo.toml");
}
