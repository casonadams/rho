use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let root = std::env::temp_dir();
    let config = Config::default();
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 8);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"read"));
    assert!(names.contains(&"write"));
    assert!(names.contains(&"edit"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"fd"));
    assert!(names.contains(&"rg"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"web_fetch"));
}
