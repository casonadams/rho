use super::*;

fn names_without_memory(tools: &[Arc<dyn EngineTool>]) -> Vec<&str> {
    tools
        .iter()
        .map(|t| t.name())
        .filter(|name| *name != "memory")
        .collect()
}

#[test]
fn builtin_tools_build_successfully() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None).unwrap();
    let names = names_without_memory(&tools);
    assert_eq!(names.len(), 11);
    for exp in [
        "read",
        "write",
        "edit",
        "undo",
        "bash",
        "fd",
        "rg",
        "web_search",
        "web_fetch",
        "todo",
        "subagent",
    ] {
        assert!(names.contains(&exp));
    }
}

#[test]
fn memory_tool_needs_a_home_and_context_files() {
    let home = Path::new("/home/tester");
    let work = std::env::temp_dir();
    let enabled = Config::default();
    assert_eq!(memory_tool(Some(home), &work, &enabled).unwrap().name(), "memory");
    assert!(memory_tool(None, &work, &enabled).is_none());
    let disabled = Config {
        no_context_files: true,
        ..Config::default()
    };
    assert!(memory_tool(Some(home), &work, &disabled).is_none());
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
    assert_eq!(names_without_memory(&tools).len(), 9);
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
