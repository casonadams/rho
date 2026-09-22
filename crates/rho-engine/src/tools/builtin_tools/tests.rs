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

#[test]
fn builtin_tools_omits_web_search_when_disabled() {
    let root = std::env::temp_dir();
    let mut config = Config::default();
    config.tools.web.search.enabled = false;
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 7);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(!names.contains(&"web_search"));
    assert!(names.contains(&"web_fetch"));
}

#[test]
fn builtin_tools_omits_web_fetch_when_disabled() {
    let root = std::env::temp_dir();
    let mut config = Config::default();
    config.tools.web.fetch.enabled = false;
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 7);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"web_search"));
    assert!(!names.contains(&"web_fetch"));
}

#[test]
fn builtin_tools_omits_both_web_tools_when_disabled() {
    let root = std::env::temp_dir();
    let mut config = Config::default();
    config.tools.web.search.enabled = false;
    config.tools.web.fetch.enabled = false;
    let tools = build_builtin_tools(&root, &config).unwrap();
    assert_eq!(tools.len(), 6);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(!names.contains(&"web_search"));
    assert!(!names.contains(&"web_fetch"));
}
