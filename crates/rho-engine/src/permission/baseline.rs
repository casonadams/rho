pub const BASELINE_TOOLS: &[&str] = &["read", "write", "edit", "fd", "rg", "web_search", "web_fetch"];

pub fn is_baseline_tool(tool: &str) -> bool {
    BASELINE_TOOLS.contains(&tool)
}
