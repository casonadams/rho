use super::*;
use crate::provider::adapter::MockAdapter;
use crate::test_support::TestDir;
use crate::tools::subagent::RunnerSpec;
use serde_json::json;

async fn run(tool: &Arc<dyn EngineTool>, args: serde_json::Value) -> crate::tools::ToolResult {
    tool.execute(args).await.unwrap()
}

fn named<'a>(tools: &'a [Arc<dyn EngineTool>], name: &str) -> &'a Arc<dyn EngineTool> {
    tools.iter().find(|tool| tool.name() == name).unwrap()
}

#[tokio::test]
async fn a_subagent_cannot_undo_the_parents_edits_and_the_parent_cannot_undo_the_subagents() {
    let dir = TestDir::new();
    let model: crate::engine::compactor::llm::ModelHandle = Arc::new(MockAdapter::text("x"));
    let (tools, subagent) = build_builtin_toolset(
        &dir,
        &Config::default(),
        Some(model),
        None,
        ToolsetServices::default(),
        &SubagentRuntime::default(),
    )
    .unwrap();
    let spec = RunnerSpec {
        agent: Some("general"),
        local_agents: &[],
        turns: None,
        model: None,
        position: None,
        task_name: None,
        persona: None,
    };
    let runner = subagent.runner_for(spec).await.unwrap();
    let child = |name: &str| runner.tool(name).unwrap().clone();
    run(named(&tools, "write"), json!({ "path": "parent.txt", "content": "p" })).await;
    run(&child("write"), json!({ "path": "child.txt", "content": "c" })).await;

    let parent_undoes_child = run(named(&tools, "undo"), json!({ "path": "child.txt" })).await;
    let child_undoes_parent = run(&child("undo"), json!({ "path": "parent.txt" })).await;
    assert!(parent_undoes_child.is_error, "{}", parent_undoes_child.content);
    assert!(child_undoes_parent.is_error, "{}", child_undoes_parent.content);
    assert!(dir.join("child.txt").exists() && dir.join("parent.txt").exists());

    assert!(!run(&child("undo"), json!({ "path": "child.txt" })).await.is_error);
    assert!(
        !run(named(&tools, "undo"), json!({ "path": "parent.txt" }))
            .await
            .is_error
    );
}

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
    assert_eq!(names.len(), 12);
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
        "wait",
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
    assert_eq!(names_without_memory(&tools).len(), 10);
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
