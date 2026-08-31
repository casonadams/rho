use super::active_set::{ActiveToolSet, floor};
use super::types::ActiveTool;
use crate::permission::{PolicyEvaluator, PolicyFailureMode, PolicyLimits};
use async_trait::async_trait;
use rho_core::approval::{
    ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, ToolEvent, approval_context,
};
use rho_core::config::Config;
use rho_core::presentation::questions::{InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion};
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{
    ExecutionMode, NetworkAccess, OperationEffect, PathScope, PermissionCapability, PermissionDecision,
    RequestedOperation, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use rig::tool::{ToolContext, ToolErrorKind, ToolSet};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

struct ApprovalSink;

#[async_trait]
impl ApprovalEventSink for ApprovalSink {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approved
    }

    fn emit(&self, _event: ToolEvent) {}
}

struct AnswerPort;

#[async_trait]
impl InteractiveQuestionPort for AnswerPort {
    async fn ask(&self, _question: UserQuestion) -> std::result::Result<UserAnswer, rho_core::error::AppError> {
        Ok(UserAnswer::Custom("host answer".to_string()))
    }
}

struct DenyPolicy(Arc<std::sync::atomic::AtomicBool>);

#[async_trait]
impl PermissionCapability for DenyPolicy {
    fn id(&self) -> CapabilityId {
        "permission:deny-fixture".parse().unwrap()
    }

    async fn evaluate(&self, _request: RequestedOperation) -> std::result::Result<PermissionDecision, CapabilityError> {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(PermissionDecision::Deny {
            rationale: "denied by fixture policy".to_string(),
        })
    }
}

struct AllowPolicy(Arc<std::sync::atomic::AtomicBool>);

#[async_trait]
impl PermissionCapability for AllowPolicy {
    fn id(&self) -> CapabilityId {
        "permission:allow-fixture".parse().unwrap()
    }

    async fn evaluate(&self, _request: RequestedOperation) -> std::result::Result<PermissionDecision, CapabilityError> {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(PermissionDecision::Allow)
    }
}

struct RecordingSink {
    requests: std::sync::Mutex<usize>,
    events: std::sync::Mutex<Vec<ToolEvent>>,
    decision: std::sync::Mutex<ApprovalDecision>,
}

fn recording_sink(decision: ApprovalDecision) -> Arc<RecordingSink> {
    Arc::new(RecordingSink {
        requests: std::sync::Mutex::new(0),
        events: std::sync::Mutex::new(Vec::new()),
        decision: std::sync::Mutex::new(decision),
    })
}

#[async_trait]
impl ApprovalEventSink for RecordingSink {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
        *self.requests.lock().unwrap() += 1;
        self.decision.lock().unwrap().clone()
    }

    fn emit(&self, event: ToolEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn event_names(sink: &RecordingSink) -> Vec<&'static str> {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .map(|event| match event {
            ToolEvent::CallClassified { .. } => "classified",
            ToolEvent::ApprovalGranted { .. } => "granted",
            ToolEvent::ApprovalDenied { .. } => "denied",
            ToolEvent::Finished { status, .. } => match status.as_str() {
                "success" => "finished-success",
                "denied" => "finished-denied",
                _ => "finished-error",
            },
        })
        .collect()
}

pub fn fixture_descriptor(id: &str, effects: Vec<OperationEffect>) -> ToolDescriptor {
    ToolDescriptor {
        id: format!("tool:{id}").parse().unwrap(),
        description: "fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects,
        execution_mode: ExecutionMode::Sequential,
    }
}

fn fixture_tool(id: &str, effects: Vec<OperationEffect>, executed: &Arc<std::sync::atomic::AtomicBool>) -> ActiveTool {
    let descriptor = fixture_descriptor(id, effects);
    ActiveTool {
        target_id: descriptor.id.clone(),
        descriptor: descriptor.clone(),
        capability: Arc::new(FixtureTool(descriptor, Arc::clone(executed))),
    }
}

pub struct FixtureTool(pub ToolDescriptor, pub Arc<std::sync::atomic::AtomicBool>);

#[async_trait]
impl ToolCapability for FixtureTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.0.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        _request: ToolInvocationRequest,
    ) -> std::result::Result<ToolInvocationResponse, CapabilityError> {
        self.1.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(ToolInvocationResponse {
            content: "executed".to_string(),
            is_error: false,
            structured_content: None,
        })
    }
}

struct DispatchFixture {
    tool: ActiveTool,
    policies: Vec<Arc<dyn PermissionCapability>>,
}

impl DispatchFixture {
    fn tool(tool: ActiveTool) -> Self {
        Self {
            tool,
            policies: Vec::new(),
        }
    }

    fn with_policy(mut self, policy: Arc<dyn PermissionCapability>) -> Self {
        self.policies.push(policy);
        self
    }
}

fn tool_set(config: &Config, base_dir: &Path, fixture: DispatchFixture) -> (ToolSet, Arc<PolicyEvaluator>) {
    let DispatchFixture { tool, policies } = fixture;
    let policies = Arc::new(PolicyEvaluator::spawn(
        policies,
        PolicyFailureMode::Deny,
        PolicyLimits::default(),
    ));
    let active = ActiveToolSet {
        tools: BTreeMap::from([(tool.target_id.name().to_string(), tool)]),
        contexts: BTreeMap::new(),
        commands: BTreeMap::new(),
        lifecycles: Vec::new(),
        floor: Arc::new(floor(config, base_dir).unwrap()),
        policies: Arc::clone(&policies),
    };
    (ToolSet::from_dynamic_tools(active.into_rig_tools()), policies)
}

#[tokio::test]
async fn builtin_interaction_routes_through_the_host_context() {
    let config = Config::default();
    let active = ActiveToolSet::builtins(&config, &std::env::temp_dir()).unwrap();
    assert!(active.active_contexts().is_empty());
    assert!(active.active_commands().is_empty());
    assert!(active.active_lifecycles().is_empty());
    let tools = ToolSet::from_dynamic_tools(active.into_rig_tools());
    let mut context = ToolContext::new();
    context.insert(QuestionPort::new(AnswerPort));

    let result = tools
        .execute("ask_user", r#"{"question":"Continue?"}"#, &mut context)
        .await;

    assert_eq!(result.output().as_text(), Some("host answer"));
}

#[tokio::test]
async fn approved_external_addition_executes_through_the_host_boundary() {
    let root = std::env::temp_dir();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:external".parse().unwrap(),
        description: "external fixture".to_string(),
        argument_schema: serde_json::json!({"type": "object"}),
        prompt_guidance: String::new(),
        effects: Vec::new(),
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:external".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let capability = ApprovalCapability::new(false, Arc::new(ApprovalSink));
    capability.grant_once("external", &serde_json::json!({"value": 1}));
    let mut context = approval_context(capability);
    let result = tools.execute("external", r#"{"value":1}"#, &mut context).await;

    assert_eq!(result.output().as_text(), Some("executed"));
    assert!(executed.load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn schema_and_workspace_floor_reject_before_external_execution() {
    let root = std::env::temp_dir().join(format!("dispatch_root_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:external-write".parse().unwrap(),
        description: "write fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects: vec![OperationEffect::WritePath {
            scope: PathScope::Workspace,
        }],
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:external-write".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let mut context = ToolContext::new();

    let malformed = tools.execute("external-write", "{}", &mut context).await;
    assert!(malformed.is_error_kind(ToolErrorKind::InvalidArgs));
    let escaping = tools
        .execute("external-write", r#"{"path":"../outside"}"#, &mut context)
        .await;
    assert!(escaping.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_is_denied_before_external_execution() {
    let root = std::env::temp_dir().join(format!("dispatch_symlink_{}", uuid::Uuid::new_v4()));
    let outside = std::env::temp_dir().join(format!("dispatch_outside_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:external-write".parse().unwrap(),
        description: "write fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects: vec![OperationEffect::WritePath {
            scope: PathScope::Workspace,
        }],
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:external-write".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let result = tools
        .execute(
            "external-write",
            r#"{"path":"escape/out.txt"}"#,
            &mut ToolContext::new(),
        )
        .await;

    assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn protected_host_storage_is_denied_before_read_dispatch() {
    let root = std::env::temp_dir().join(format!("dispatch_protected_{}", uuid::Uuid::new_v4()));
    let protected = root.join("rho");
    std::fs::create_dir_all(&protected).unwrap();
    let config = Config {
        config_dir: protected.clone(),
        ..Config::default()
    };
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:external-read".parse().unwrap(),
        description: "read fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects: vec![OperationEffect::ReadPath {
            scope: PathScope::Explicit,
        }],
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &config,
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:external-read".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let arguments = serde_json::json!({"path": protected.join("credentials.json")}).to_string();
    let result = tools
        .execute("external-read", &arguments, &mut ToolContext::new())
        .await;

    assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn replacement_cannot_weaken_builtin_safety_floor() {
    let root = std::env::temp_dir().join(format!("dispatch_replace_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:replacement-write".parse().unwrap(),
        description: "replacement".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects: Vec::new(),
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:write".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let result = tools
        .execute("write", r#"{"path":"../outside"}"#, &mut ToolContext::new())
        .await;

    assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn private_network_floor_rejects_before_external_execution() {
    let root = std::env::temp_dir();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = ToolDescriptor {
        id: "tool:external-fetch".parse().unwrap(),
        description: "fetch fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["url"],
            "properties": {"url": {"type": "string"}}
        }),
        prompt_guidance: String::new(),
        effects: vec![OperationEffect::Network {
            access: NetworkAccess::PublicInternet,
        }],
        execution_mode: ExecutionMode::Sequential,
    };
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:external-fetch".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        }),
    );
    let result = tools
        .execute(
            "external-fetch",
            r#"{"url":"http://127.0.0.1/private"}"#,
            &mut ToolContext::new(),
        )
        .await;
    assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn a_policy_deny_blocks_an_allowed_tool_without_side_effect() {
    let root = std::env::temp_dir();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(fixture_tool("read", Vec::new(), &executed)).with_policy(Arc::new(DenyPolicy(Arc::new(
            std::sync::atomic::AtomicBool::new(false),
        )))),
    );
    let result = tools
        .execute("read", r#"{"path":"notes.md"}"#, &mut ToolContext::new())
        .await;

    assert!(result.is_refused());
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn a_policy_allow_cannot_bypass_the_host_floor_or_idle_evaluation() {
    let root = std::env::temp_dir().join(format!("dispatch_policy_floor_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let consulted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descriptor = fixture_descriptor(
        "replacement-write",
        vec![OperationEffect::WritePath {
            scope: PathScope::Workspace,
        }],
    );
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(ActiveTool {
            target_id: "tool:write".parse().unwrap(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
        })
        .with_policy(Arc::new(AllowPolicy(Arc::clone(&consulted)))),
    );
    let result = tools
        .execute("write", r#"{"path":"../outside"}"#, &mut ToolContext::new())
        .await;

    assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
    assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!consulted.load(std::sync::atomic::Ordering::Relaxed));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn dispatch_emits_exactly_one_lifecycle_history_per_call() {
    let root = std::env::temp_dir().join(format!("dispatch_events_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tools, _policies) = tool_set(
        &Config::default(),
        &root,
        DispatchFixture::tool(fixture_tool(
            "write",
            vec![OperationEffect::WritePath {
                scope: PathScope::Workspace,
            }],
            &executed,
        )),
    );

    let approved = recording_sink(ApprovalDecision::Approved);
    let capability = ApprovalCapability::new(false, Arc::clone(&approved) as Arc<dyn ApprovalEventSink>);
    let mut context = approval_context(capability);
    let approved_result = tools.execute("write", r#"{"path":"out.txt"}"#, &mut context).await;
    assert!(approved_result.is_success());
    assert_eq!(
        event_names(&approved),
        vec!["classified", "granted", "finished-success"],
        "exactly one classification, approval, and finish event"
    );
    assert_eq!(*approved.requests.lock().unwrap(), 1);

    let denied = recording_sink(ApprovalDecision::Denied {
        reason: "not now".to_string(),
    });
    let capability = ApprovalCapability::new(false, Arc::clone(&denied) as Arc<dyn ApprovalEventSink>);
    let mut context = approval_context(capability);
    let result = tools.execute("write", r#"{"path":"out.txt"}"#, &mut context).await;
    assert!(result.is_refused());
    assert_eq!(event_names(&denied), vec!["classified", "denied", "finished-denied"]);
    assert_eq!(*denied.requests.lock().unwrap(), 1);
    std::fs::remove_dir_all(root).unwrap();
}
