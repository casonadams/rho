use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let root = std::env::temp_dir();
    let config = Config::default();
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 9);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"read"));
    assert!(names.contains(&"write"));
    assert!(names.contains(&"edit"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"fd"));
    assert!(names.contains(&"rg"));
    assert!(names.contains(&"outline"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn test_outline_tool_execution_via_builtin_tools() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let main_rs = temp.path().join("main.rs");
    std::fs::write(
        &main_rs,
        "pub struct Config;\nimpl Config {\n    pub fn load() -> Self { Self }\n}",
    )
    .unwrap();

    let tools = build_builtin_tools(temp.path(), &config).unwrap();
    let mut tool_set = rig::tool::ToolSet::default();
    for tool in tools {
        tool_set.add_dynamic_tool(tool);
    }

    let mut tool_context = rig::tool::ToolContext::new();
    let result = tool_set
        .execute("outline", r#"{"path": "main.rs"}"#, &mut tool_context)
        .await;

    assert!(result.is_success());
    let text = result.output().as_text().expect("text output");
    assert!(text.contains("main.rs:"));
    assert!(text.contains("pub struct Config"));
    assert!(text.contains("pub fn load() -> Self"));
}
