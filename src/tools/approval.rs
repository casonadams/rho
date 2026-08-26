use crate::tools::bash_ast::RiskTier;
use crate::tools::policy::{ExecutionClass, ToolExecutionPolicy};
use async_trait::async_trait;
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent};
use rig::tool::{ToolContext, ToolExecutionError};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const DENIED_MESSAGE: &str = "Operation denied by user; no changes were made.";

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: Value,
    pub tier: RiskTier,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolEvent {
    CallClassified {
        internal_call_id: String,
        tool_name: String,
        arguments: Value,
        class: ExecutionClass,
    },
    ApprovalGranted {
        internal_call_id: String,
        tool_name: String,
    },
    ApprovalDenied {
        internal_call_id: String,
        tool_name: String,
    },
    Finished {
        internal_call_id: String,
        tool_name: String,
        arguments: Value,
        output: String,
        status: String,
    },
}

#[async_trait]
pub trait ApprovalEventSink: Send + Sync {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalDecision;

    fn emit(&self, _event: ToolEvent) {}
}

#[derive(Clone)]
pub struct ApprovalCapability {
    inner: Arc<ApprovalCapabilityInner>,
}

struct ApprovalCapabilityInner {
    auto_approve: bool,
    sink: Arc<dyn ApprovalEventSink>,
    grants: Mutex<HashMap<String, usize>>,
    denials: Mutex<HashMap<String, String>>,
}

impl ApprovalCapability {
    pub fn new(auto_approve: bool, sink: Arc<dyn ApprovalEventSink>) -> Self {
        Self {
            inner: Arc::new(ApprovalCapabilityInner {
                auto_approve,
                sink,
                grants: Mutex::new(HashMap::new()),
                denials: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn grant_once(&self, tool_name: &str, arguments: &Value) {
        let Some(key) = approval_key(tool_name, arguments) else {
            return;
        };
        if let Ok(mut grants) = self.inner.grants.lock() {
            *grants.entry(key).or_default() += 1;
        }
    }

    fn deny_once(&self, request: ApprovalRequest, reason: String) {
        let Some(key) = approval_key(&request.tool_name, &request.arguments) else {
            return;
        };
        if let Ok(mut denials) = self.inner.denials.lock() {
            denials.insert(key, denial_message(reason));
        }
    }

    fn authorize(&self, tool_name: &str, arguments: &Value) -> Result<(), ToolExecutionError> {
        if self.inner.auto_approve {
            return Ok(());
        }
        let Some(key) = approval_key(tool_name, arguments) else {
            return Err(ToolExecutionError::refused(DENIED_MESSAGE));
        };
        if consume_grant(&self.inner.grants, &key) {
            return Ok(());
        }
        let reason = self
            .inner
            .denials
            .lock()
            .ok()
            .and_then(|mut denials| denials.remove(&key))
            .unwrap_or_else(|| DENIED_MESSAGE.to_string());
        Err(ToolExecutionError::refused(reason))
    }
}

#[derive(Clone)]
pub struct ApprovalHook {
    capability: ApprovalCapability,
}

impl ApprovalHook {
    pub fn new(capability: ApprovalCapability) -> Self {
        Self { capability }
    }
}

impl AgentHook for ApprovalHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let arguments = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        let class = ToolExecutionPolicy::classify(event.tool_name, &arguments);
        self.capability.inner.sink.emit(ToolEvent::CallClassified {
            internal_call_id: event.internal_call_id.to_string(),
            tool_name: event.tool_name.to_string(),
            arguments: arguments.clone(),
            class: class.clone(),
        });

        if class.allows_without_approval() || self.capability.inner.auto_approve {
            return ToolCallAction::Run;
        }

        let ExecutionClass::ApprovalRequired { tier, reasons } = class else {
            return ToolCallAction::Run;
        };
        let request = ApprovalRequest {
            tool_name: event.tool_name.to_string(),
            arguments: arguments.clone(),
            tier,
            reasons,
        };
        let decision = self.capability.inner.sink.request_approval(request.clone()).await;
        match decision {
            ApprovalDecision::Approved => {
                self.capability.grant_once(event.tool_name, &arguments);
                self.capability.inner.sink.emit(ToolEvent::ApprovalGranted {
                    internal_call_id: event.internal_call_id.to_string(),
                    tool_name: event.tool_name.to_string(),
                });
            }
            ApprovalDecision::Denied { reason } => {
                self.capability.deny_once(request, reason);
                self.capability.inner.sink.emit(ToolEvent::ApprovalDenied {
                    internal_call_id: event.internal_call_id.to_string(),
                    tool_name: event.tool_name.to_string(),
                });
            }
        }
        ToolCallAction::Run
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        self.capability.inner.sink.emit(ToolEvent::Finished {
            internal_call_id: event.internal_call_id.to_string(),
            tool_name: event.tool_name.to_string(),
            arguments: serde_json::from_str(event.args).unwrap_or(Value::Null),
            output: event.presentation.render(),
            status: event.raw_result.status_name().to_string(),
        });
        ToolResultAction::Keep
    }
}

pub fn approval_context(capability: ApprovalCapability) -> ToolContext {
    let mut context = ToolContext::new();
    context.insert(capability);
    context
}

pub fn enforce_approval<T>(context: &ToolContext, tool_name: &str, arguments: &T) -> Result<(), ToolExecutionError>
where
    T: Serialize,
{
    let arguments = serde_json::to_value(arguments)
        .map_err(|_| ToolExecutionError::invalid_args("Tool arguments could not be validated safely"))?;
    if ToolExecutionPolicy::classify(tool_name, &arguments).allows_without_approval() {
        return Ok(());
    }

    let capability = context
        .get::<ApprovalCapability>()
        .ok_or_else(|| ToolExecutionError::refused("Approval context is missing; no operation was executed"))?;
    capability.authorize(tool_name, &arguments)
}

fn consume_grant(grants: &Mutex<HashMap<String, usize>>, key: &str) -> bool {
    let Ok(mut grants) = grants.lock() else {
        return false;
    };
    let Some(count) = grants.get_mut(key) else {
        return false;
    };
    *count -= 1;
    if *count == 0 {
        grants.remove(key);
    }
    true
}

fn denial_message(reason: String) -> String {
    if reason.trim().is_empty() {
        DENIED_MESSAGE.to_string()
    } else {
        format!("Operation denied by user: {} No changes were made.", reason.trim())
    }
}

fn approval_key(tool_name: &str, arguments: &Value) -> Option<String> {
    let arguments = ToolExecutionPolicy::canonical_arguments(tool_name, arguments)?;
    serde_json::to_string(&arguments)
        .ok()
        .map(|arguments| format!("{tool_name}:{arguments}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::bash::BashTool;
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

    #[async_trait]
    impl ApprovalEventSink for FakeSink {
        async fn request_approval(&self, request: ApprovalRequest) -> ApprovalDecision {
            self.requests.lock().unwrap().push(request);
            self.decision.clone()
        }

        fn emit(&self, event: ToolEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    async fn run_tool<T>(tool: T, arguments: Value, capability: ApprovalCapability) -> MockCompletionModel
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
}
