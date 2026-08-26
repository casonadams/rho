use crate::tools::approval::{
    approval_context, ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalHook, ApprovalRequest, ToolEvent,
};
use crate::tools::bash::BashTool;
use crate::tools::bash_ast::RiskTier;
use crate::tools::edit::EditTool;
use crate::tools::read::ReadTool;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::WriteTool;
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use rig::tool::{Tool, ToolContext, ToolSet};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct FakeSink {
    decision: ApprovalDecision,
    requests: Mutex<Vec<ApprovalRequest>>,
    events: Mutex<Vec<ToolEvent>>,
}

impl FakeSink {
    fn new(decision: ApprovalDecision) -> Arc<Self> {
        Arc::new(Self {
            decision,
            requests: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
        })
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn last_request(&self) -> ApprovalRequest {
        self.requests.lock().unwrap().last().unwrap().clone()
    }

    fn statuses(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                ToolEvent::Finished { status, .. } => Some(status.clone()),
                _ => None,
            })
            .collect()
    }
}

#[rig::async_trait::async_trait]
impl ApprovalEventSink for FakeSink {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalDecision {
        self.requests.lock().unwrap().push(request);
        self.decision.clone()
    }

    fn emit(&self, event: ToolEvent) {
        self.events.lock().unwrap().push(event);
    }
}

async fn run_tool<T>(tool: T, arguments: serde_json::Value, capability: ApprovalCapability) -> MockCompletionModel
where
    T: Tool + 'static,
{
    let model = MockCompletionModel::new([
        MockTurn::tool_call("call-1", T::NAME, arguments),
        MockTurn::text("done"),
    ]);
    let agent = AgentBuilder::new(model.clone())
        .tool(tool)
        .add_hook(ApprovalHook::new(capability.clone()))
        .build();
    agent
        .runner("use the tool")
        .tool_context(approval_context(capability))
        .tool_concurrency(1)
        .max_turns(3)
        .run()
        .await
        .unwrap();
    model
}

#[tokio::test]
async fn denied_write_has_no_filesystem_side_effect_and_is_model_visible() {
    let dir = temp_dir("denied_write");
    let path = dir.join("nested/output.txt");
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "not now".to_string(),
    });
    let model = run_tool(
        WriteTool::new(&dir),
        json!({"path": path, "content": "forbidden"}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert!(!path.exists());
    assert_eq!(sink.request_count(), 1);
    assert_eq!(sink.statuses(), ["denied"]);
    let model_request = format!("{:?}", model.requests()[1]);
    assert!(model_request.contains("not now"));
    assert!(model_request.contains("No changes were made"));
}

#[tokio::test]
async fn contained_write_executes_without_approval_prompt() {
    let base = std::env::current_dir().unwrap();
    let path = base.join(format!(".approval_contained_{}", uuid::Uuid::new_v4()));
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "must not be consulted".to_string(),
    });
    run_tool(
        WriteTool::new(&base),
        json!({"path": path, "content": "contained"}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "contained");
    assert_eq!(sink.request_count(), 0);
    tokio::fs::remove_file(path).await.unwrap();
}

#[tokio::test]
async fn approved_write_executes_once() {
    let dir = temp_dir("approved_write");
    let path = dir.join("output.txt");
    let sink = FakeSink::new(ApprovalDecision::Approved);
    run_tool(
        WriteTool::new(&dir),
        json!({"path": path, "content": "approved"}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "approved");
    assert_eq!(sink.request_count(), 1);
    assert_eq!(sink.statuses(), ["success"]);
}

#[tokio::test]
async fn denied_edit_leaves_existing_file_unchanged() {
    let dir = temp_dir("denied_edit");
    let path = dir.join("input.txt");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(&path, "old").await.unwrap();
    let sink = FakeSink::new(ApprovalDecision::Denied { reason: String::new() });
    run_tool(
        EditTool::new(&dir),
        json!({"path": path, "edits": [{"oldText": "old", "newText": "new"}]}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "old");
    assert_eq!(sink.statuses(), ["denied"]);
}

#[tokio::test]
async fn contained_edit_executes_without_approval_prompt() {
    let base = std::env::current_dir().unwrap();
    let path = base.join(format!(".approval_edit_contained_{}", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, "old").await.unwrap();
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "must not be consulted".to_string(),
    });
    run_tool(
        EditTool::new(&base),
        json!({"path": path, "edits": [{"oldText": "old", "newText": "new"}]}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "new");
    assert_eq!(sink.request_count(), 0);
    tokio::fs::remove_file(path).await.unwrap();
}

#[tokio::test]
async fn approved_edit_changes_existing_file() {
    let dir = temp_dir("approved_edit");
    let path = dir.join("input.txt");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(&path, "old").await.unwrap();
    let sink = FakeSink::new(ApprovalDecision::Approved);
    run_tool(
        EditTool::new(&dir),
        json!({"path": path, "edits": [{"oldText": "old", "newText": "new"}]}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "new");
    assert_eq!(sink.request_count(), 1);
    assert_eq!(sink.statuses(), ["success"]);
}

#[cfg(unix)]
#[tokio::test]
async fn denied_mutating_bash_has_no_process_side_effect() {
    let dir = temp_dir("denied_bash");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let marker = dir.join("marker");
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "unsafe".to_string(),
    });
    run_tool(
        BashTool::new(&dir),
        json!({"command": format!("touch {}", marker.display())}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert!(!marker.exists());
    assert_eq!(sink.request_count(), 1);
    let request = sink.last_request();
    assert_eq!(request.tier, RiskTier::Mutating);
    assert!(!request.reasons.is_empty());
    assert_eq!(sink.statuses(), ["denied"]);
}

#[cfg(unix)]
#[tokio::test]
async fn denied_high_risk_bash_includes_warning_metadata() {
    let dir = temp_dir("denied_high_risk_bash");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let target = dir.join("target");
    tokio::fs::create_dir_all(&target).await.unwrap();
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "destructive".to_string(),
    });
    run_tool(
        BashTool::new(&dir),
        json!({"command": format!("rm -rf {}", target.display())}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert!(target.exists());
    let request = sink.last_request();
    assert_eq!(request.tier, RiskTier::HighRisk);
    assert!(request.reasons.iter().any(|reason| reason.contains("HIGH RISK")));
}

#[cfg(unix)]
#[tokio::test]
async fn approved_mutating_bash_executes_once() {
    let dir = temp_dir("approved_bash");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let marker = dir.join("marker");
    let sink = FakeSink::new(ApprovalDecision::Approved);
    run_tool(
        BashTool::new(&dir),
        json!({"command": format!("touch {}", marker.display())}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert!(marker.exists());
    assert_eq!(sink.request_count(), 1);
    assert_eq!(sink.statuses(), ["success"]);
}

#[tokio::test]
async fn read_only_bash_and_read_do_not_request_approval() {
    let dir = temp_dir("read_only");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let path = dir.join("input.txt");
    tokio::fs::write(&path, "visible").await.unwrap();
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "deny all prompts".to_string(),
    });

    run_tool(
        BashTool::new(&dir),
        json!({"command": "printf visible"}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;
    run_tool(
        ReadTool::new(&dir),
        json!({"path": path}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(sink.request_count(), 0);
    assert_eq!(sink.statuses(), ["success", "success"]);
}

#[tokio::test]
async fn web_tools_do_not_request_approval() {
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "deny all prompts".to_string(),
    });
    let http = HttpClient::new(false).unwrap();
    run_tool(
        WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(0),
            WebSearchConfig {
                region: "wt-wt".to_string(),
                timeout_sec: 1,
            },
        ),
        json!({"query": ""}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;
    run_tool(
        WebFetchTool::new(
            http,
            FetchCache::new(60, 4),
            WebFetchConfig {
                timeout_sec: 1,
                max_bytes: 1024,
                default_limit: 20,
            },
        ),
        json!({"url": "http://127.0.0.1/private"}),
        ApprovalCapability::new(false, sink.clone()),
    )
    .await;

    assert_eq!(sink.request_count(), 0);
    assert_eq!(sink.statuses(), ["error", "error"]);
}

#[tokio::test]
async fn auto_approve_allows_mutation_without_prompt() {
    let dir = temp_dir("auto_approve");
    let path = dir.join("output.txt");
    let sink = FakeSink::new(ApprovalDecision::Denied {
        reason: "must not be consulted".to_string(),
    });
    run_tool(
        WriteTool::new(&dir),
        json!({"path": path, "content": "approved"}),
        ApprovalCapability::new(true, sink.clone()),
    )
    .await;

    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "approved");
    assert_eq!(sink.request_count(), 0);
}

#[tokio::test]
async fn direct_mutating_dispatch_without_capability_fails_closed() {
    let dir = temp_dir("missing_capability");
    let path = dir.join("output.txt");
    let mut tools = ToolSet::default();
    tools.add_tool(WriteTool::new(&dir));
    let result = tools
        .execute(
            "write",
            json!({"path": path, "content": "forbidden"}).to_string(),
            &mut ToolContext::new(),
        )
        .await;

    assert!(result.is_refused());
    assert!(!path.exists());
    assert!(result.output().render().contains("Approval context is missing"));
}

#[cfg(unix)]
#[tokio::test]
async fn direct_mutating_bash_without_capability_spawns_no_process() {
    let dir = temp_dir("missing_bash_capability");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let marker = dir.join("marker");
    let mut tools = ToolSet::default();
    tools.add_tool(BashTool::new(&dir));
    let result = tools
        .execute(
            "bash",
            json!({"command": format!("touch {}", marker.display())}).to_string(),
            &mut ToolContext::new(),
        )
        .await;

    assert!(result.is_refused());
    assert!(!marker.exists());
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rust_ai_{label}_{}", uuid::Uuid::new_v4()))
}
