use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None).unwrap();
    assert_eq!(tools.len(), 9);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    for exp in [
        "read",
        "write",
        "edit",
        "bash",
        "fd",
        "rg",
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
    let search_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap();
    assert!(search_off.iter().all(|t| t.name() != "web_search"));
    assert!(search_off.iter().any(|t| t.name() == "web_fetch"));

    config.tools.web.search.enabled = true;
    config.tools.web.fetch.enabled = false;
    let fetch_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap();
    assert!(fetch_off.iter().any(|t| t.name() == "web_search"));
    assert!(fetch_off.iter().all(|t| t.name() != "web_fetch"));

    config.tools.web.search.enabled = false;
    let both_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap();
    assert!(
        both_off
            .iter()
            .all(|t| t.name() != "web_search" && t.name() != "web_fetch")
    );
}

#[test]
fn builtin_tools_omit_web_when_disabled() {
    let mut config = Config::default();
    config.tools.web.search.enabled = false;
    config.tools.web.fetch.enabled = false;
    let tools = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap();
    assert_eq!(tools.len(), 7);
}
