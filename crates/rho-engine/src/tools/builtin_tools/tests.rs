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

#[tokio::test]
async fn subagent_bash_runs_with_the_configured_direnv_options() {
    let dir = TestDir::new();
    let model: crate::engine::compactor::llm::ModelHandle = Arc::new(MockAdapter::text("x"));
    let mut config = Config::default();
    config.tools.bash.direnv = true;
    config.tools.bash.direnv_timeout_ms = 1234;
    let configured = crate::tools::bash::DirenvOptions {
        enabled: true,
        timeout_ms: 1234,
    };
    let (tools, subagent) = build_builtin_toolset(
        &dir,
        &config,
        Some(model),
        None,
        ToolsetServices::default(),
        &SubagentRuntime::default(),
    )
    .unwrap();
    let runner = subagent
        .runner_for(RunnerSpec {
            agent: Some("general"),
            local_agents: &[],
            turns: None,
            model: None,
            position: None,
            task_name: None,
            persona: None,
        })
        .await
        .unwrap();

    assert_eq!(named(&tools, "bash").direnv_options(), Some(configured));
    assert_eq!(runner.tool("bash").unwrap().direnv_options(), Some(configured));
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
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None)
        .unwrap()
        .tools;
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
    let search_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap().tools;
    assert!(search_off.iter().all(|t| t.name() != "web_search"));
    assert!(search_off.iter().any(|t| t.name() == "web_fetch"));

    config.tools.web.search.enabled = true;
    config.tools.web.fetch.enabled = false;
    let fetch_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap().tools;
    assert!(fetch_off.iter().any(|t| t.name() == "web_search"));
    assert!(fetch_off.iter().all(|t| t.name() != "web_fetch"));

    config.tools.web.search.enabled = false;
    let both_off = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap().tools;
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
    let tools = build_builtin_tools(&std::env::temp_dir(), &config, None).unwrap().tools;
    assert_eq!(names_without_memory(&tools).len(), 10);
}

#[test]
fn only_read_only_builtin_tools_run_concurrently() {
    let tools = build_builtin_tools(&std::env::temp_dir(), &Config::default(), None)
        .unwrap()
        .tools;
    let args = serde_json::json!({});
    let mut parallel: Vec<_> = tools
        .iter()
        .filter(|t| t.name() != "subagent" && t.concurrency_safe(&args))
        .map(|t| t.name())
        .collect();
    parallel.sort_unstable();
    assert_eq!(parallel, ["fd", "read", "rg", "web_fetch", "web_search"]);
}

struct InteractivePresenterStub {
    interactive: bool,
}

#[async_trait::async_trait]
impl rho_harness_core::presentation::Presenter for InteractivePresenterStub {
    fn write_output(&self, _: &str) {}
    fn print_welcome(&self, _: &rho_harness_core::presentation::WelcomeDisplay) {}
    fn print_session_status(&self, _: &rho_harness_core::presentation::SessionStatus) {}
    fn print_notice(&self, _: &str) {}
    fn print_user_block(&self, _: &str) {}
    fn print_token(&self, _: &str) {}
    fn print_thinking_token(&self, _: &str) {}
    fn finish_tool_line(&self, _: rho_harness_core::presentation::ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        self.interactive
    }
    fn start_spinner(&self, _: &str) -> rho_harness_core::presentation::ActivityToken {
        rho_harness_core::presentation::activity_token(|| {})
    }
    fn start_tool_spinner(&self, _: &str, _: &serde_json::Value) -> rho_harness_core::presentation::ActivityToken {
        rho_harness_core::presentation::activity_token(|| {})
    }
    fn start_tool_run(&self, _: &str, _: &serde_json::Value) {}
    fn stream_port(&self) -> rho_harness_core::presentation::ToolStreamPort {
        rho_harness_core::presentation::ToolStreamPort::new(None)
    }
}

#[tokio::test]
async fn ask_tool_is_registered_only_with_interactive_presenter_and_omitted_from_subagents() {
    let dir = TestDir::new();
    let model: crate::engine::compactor::llm::ModelHandle = Arc::new(MockAdapter::text("x"));

    let (headless_tools, _) = build_builtin_toolset(
        &dir,
        &Config::default(),
        Some(model.clone()),
        None,
        ToolsetServices {
            presenter: Some(Arc::new(InteractivePresenterStub { interactive: false })),
            ..ToolsetServices::default()
        },
        &SubagentRuntime::default(),
    )
    .unwrap();
    assert!(headless_tools.iter().all(|t| t.name() != "ask"));

    let (interactive_tools, subagent) = build_builtin_toolset(
        &dir,
        &Config::default(),
        Some(model),
        None,
        ToolsetServices {
            presenter: Some(Arc::new(InteractivePresenterStub { interactive: true })),
            ..ToolsetServices::default()
        },
        &SubagentRuntime::default(),
    )
    .unwrap();
    assert!(interactive_tools.iter().any(|t| t.name() == "ask"));

    let runner = subagent
        .runner_for(RunnerSpec {
            agent: Some("general"),
            local_agents: &[],
            turns: None,
            model: None,
            position: None,
            task_name: None,
            persona: None,
        })
        .await
        .unwrap();
    assert!(runner.tool("ask").is_none());
}
