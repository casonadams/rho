use crate::permission::path::{
    extract_mcp_path, extract_mcp_targets, extract_tool_path, is_path_outside_working_dir, is_safe_system_path,
    path_policy_values,
};
use serde_json::json;
use std::path::Path;

#[test]
fn path_module_normalization_and_containment() {
    let ws = Path::new("/ws");
    assert!(is_safe_system_path("/dev/null"));
    assert!(is_safe_system_path("/dev/stderr"));
    assert!(!is_safe_system_path("/tmp/foo"));

    assert!(!is_path_outside_working_dir("src/main.rs", Some(ws)));
    assert!(!is_path_outside_working_dir("src/../src/lib.rs", Some(ws)));
    assert!(!is_path_outside_working_dir("/dev/null", Some(ws)));
    assert!(is_path_outside_working_dir("/etc/passwd", Some(ws)));
    assert!(is_path_outside_working_dir("../sibling/file", Some(ws)));
    assert!(is_path_outside_working_dir("a/../../etc/passwd", Some(ws)));

    let values = path_policy_values("src/main.rs", Some(ws));
    assert!(values.contains(&"src/main.rs".to_string()));

    let tool_args = json!({"path": "src/main.rs", "old_text": "foo"});
    assert_eq!(extract_tool_path("read", &tool_args), Some("src/main.rs".to_string()));
    assert_eq!(extract_tool_path("bash", &tool_args), None);

    let mcp_args = json!({"server": "playwright", "tool": "navigate", "arguments": {"path": "/tmp/test.html"}});
    assert_eq!(extract_mcp_path(&mcp_args), Some("/tmp/test.html".to_string()));
    let targets = extract_mcp_targets(&mcp_args);
    assert_eq!(targets, vec!["playwright:navigate", "playwright"]);
}
