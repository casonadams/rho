use crate::permission::path::{
    extract_mcp_path, extract_mcp_targets, extract_tool_path, is_path_outside_working_dir, is_safe_system_path,
    path_policy_values,
};
use serde_json::json;
use std::path::Path;

#[test]
fn path_module_system_safety() {
    assert!(is_safe_system_path("/dev/null") && is_safe_system_path("/dev/stderr"));
    assert!(!is_safe_system_path("/tmp/foo"));
}

#[test]
fn path_module_working_dir_containment() {
    let ws = Path::new("/ws");
    for path in ["src/main.rs", "src/../src/lib.rs", "/dev/null"] {
        assert!(!is_path_outside_working_dir(path, Some(ws)));
    }
    for path in ["/etc/passwd", "../sibling/file", "a/../../etc/passwd"] {
        assert!(is_path_outside_working_dir(path, Some(ws)));
    }
}

#[test]
fn path_module_tool_extraction() {
    let ws = Path::new("/ws");
    assert!(path_policy_values("src/main.rs", Some(ws)).contains(&"src/main.rs".to_string()));

    let tool_args = json!({"path": "src/main.rs", "old_text": "foo"});
    assert_eq!(extract_tool_path("read", &tool_args), Some("src/main.rs".to_string()));
    assert_eq!(extract_tool_path("bash", &tool_args), None);
}

#[test]
fn path_module_mcp_extraction() {
    let mcp_args = json!({"server": "playwright", "tool": "navigate", "arguments": {"path": "/tmp/test.html"}});
    assert_eq!(extract_mcp_path(&mcp_args), Some("/tmp/test.html".to_string()));
    assert_eq!(
        extract_mcp_targets(&mcp_args),
        vec!["playwright:navigate", "playwright"]
    );
}
