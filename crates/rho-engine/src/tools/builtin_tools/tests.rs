use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let root = std::env::temp_dir();
    let config = Config::default();
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 8);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    for expected in ["read", "write", "edit", "bash", "fd", "rg", "web_search", "web_fetch"] {
        assert!(names.contains(&expected));
    }
}
