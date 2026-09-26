use crate::permission::baseline::is_baseline_tool;

#[test]
fn baseline_tools_allowed() {
    let tools = ["read", "write", "edit", "fd", "rg", "web_search", "web_fetch"];
    for t in tools {
        assert!(is_baseline_tool(t));
    }
    assert!(!is_baseline_tool("unknown_tool"));
    assert!(!is_baseline_tool("bash"));
    assert!(!is_baseline_tool("grep"));
    assert!(!is_baseline_tool("ls"));
}
