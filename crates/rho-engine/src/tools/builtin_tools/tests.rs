use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default()).unwrap();
    assert_eq!(tools.len(), 10);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    for exp in [
        "read",
        "write",
        "edit",
        "bash",
        "fd",
        "rg",
        "lsp",
        "web_search",
        "web_fetch",
        "subagent",
    ] {
        assert!(names.contains(&exp));
    }
}

#[test]
fn builtin_tools_web_flags_control_inclusion() {
    let mut config = Config::default();
    config.tools.web.search.enabled = false;
    let search_off = build_builtin_tools(&std::env::temp_dir(), &config).unwrap();
    assert!(search_off.iter().all(|t| t.name() != "web_search"));
    assert!(search_off.iter().any(|t| t.name() == "web_fetch"));

    config.tools.web.search.enabled = true;
    config.tools.web.fetch.enabled = false;
    let fetch_off = build_builtin_tools(&std::env::temp_dir(), &config).unwrap();
    assert!(fetch_off.iter().any(|t| t.name() == "web_search"));
    assert!(fetch_off.iter().all(|t| t.name() != "web_fetch"));

    config.tools.web.search.enabled = false;
    let both_off = build_builtin_tools(&std::env::temp_dir(), &config).unwrap();
    assert!(
        both_off
            .iter()
            .all(|t| t.name() != "web_search" && t.name() != "web_fetch")
    );
}

#[test]
fn native_builtin_tools_build_successfully() {
    let tools = build_native_builtin_tools(&std::env::temp_dir(), &Config::default()).unwrap();
    assert_eq!(tools.len(), 10);
    assert!(tools.iter().any(|t| t.name() == "bash"));
}

#[test]
fn native_builtin_tools_omits_web_when_disabled() {
    let mut config = Config::default();
    config.tools.web.search.enabled = false;
    config.tools.web.fetch.enabled = false;
    let tools = build_native_builtin_tools(&std::env::temp_dir(), &config).unwrap();
    assert_eq!(tools.len(), 8);
}
