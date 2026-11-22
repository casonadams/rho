use super::*;

#[test]
fn builtin_tools_build_successfully() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None).unwrap();
    assert_eq!(tools.len(), 13);
    let names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    for exp in [
        "read",
        "write",
        "edit",
        "undo",
        "bash",
        "fd",
        "rg",
        "terminal",
        "web_search",
        "web_fetch",
        "todo",
        "subagent",
        "memory",
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
    assert_eq!(tools.len(), 11);
}

#[test]
fn only_read_only_builtin_tools_run_concurrently() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None).unwrap();
    let args = serde_json::json!({});
    let mut parallel: Vec<_> = tools
        .iter()
        .filter(|t| t.name() != "subagent" && t.concurrency_safe(&args))
        .map(|t| t.name())
        .collect();
    parallel.sort_unstable();
    assert_eq!(parallel, ["fd", "read", "rg", "web_fetch", "web_search"]);
}
