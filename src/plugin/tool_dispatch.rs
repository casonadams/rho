use crate::config::Config;
use crate::error::Result;
use crate::plugin::builtin_tools::{BuiltinToolCatalog, DECLARATIONS};
use crate::plugin::capability::{CapabilityError, CapabilityId, CapabilityKind, PluginId, PluginOrigin};
use crate::plugin::contract::{
    InteractionRequest, InteractionResponse, InvocationContext, OperationEffect, PermissionCapability,
    PermissionDecision, RequestedOperation, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
};
use crate::plugin::external::ExternalPlugin;
use crate::plugin::loader::{ConfiguredStatus, PluginLoader};
use crate::plugin::permission::{PermissionRequest, PolicyEvaluator, PolicyFailureMode, PolicyLimits};
use crate::plugin::process::ProcessLimits;
use crate::plugin::resolver::{CapabilityPlugin, CapabilityResolver};
use crate::plugin::safety_floor::{FloorDenial, FloorRequest, SafetyFloor};
use crate::tools::RiskTier;
use crate::tools::approval::{
    ApprovalCapability, ApprovalDecision, ApprovalRequest, DispatchedCall, DispatchedResult, ToolEvent,
    emit_tool_finished, format_denial,
};
use crate::tools::ask_user::{QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
use crate::tools::policy::ToolExecutionPolicy;
use crate::tools::web::HttpClient;
use async_trait::async_trait;
use rig::tool::{DynamicTool, ToolContext, ToolExecutionError, ToolOutput};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
struct ActiveTool {
    target_id: CapabilityId,
    descriptor: ToolDescriptor,
    capability: Arc<dyn ToolCapability>,
}

struct DispatchContext<'a> {
    floor: &'a SafetyFloor,
    policies: &'a PolicyEvaluator,
    tool: &'a mut ToolContext,
}

#[derive(Clone)]
pub struct ActiveToolSet {
    tools: BTreeMap<String, ActiveTool>,
    floor: Arc<SafetyFloor>,
    policies: Arc<PolicyEvaluator>,
}

impl ActiveToolSet {
    pub fn builtins(config: &Config, base_dir: &Path) -> Result<Self> {
        let catalog = BuiltinToolCatalog::new(base_dir, config)?;
        let capabilities = catalog.into_capabilities();
        let tools = capabilities
            .into_iter()
            .map(|(id, capability)| {
                let descriptor = capability.descriptor();
                (
                    id.name().to_string(),
                    ActiveTool {
                        target_id: id,
                        descriptor,
                        capability,
                    },
                )
            })
            .collect();
        Ok(Self {
            tools,
            floor: Arc::new(floor(config, base_dir)?),
            policies: Arc::new(PolicyEvaluator::spawn(
                Vec::new(),
                PolicyFailureMode::Deny,
                PolicyLimits::default(),
            )),
        })
    }

    pub async fn load(config: &Config, base_dir: &Path) -> Result<Self> {
        let builtins = BuiltinToolCatalog::new(base_dir, config)?.into_capabilities();
        let mut external_plugins = BTreeMap::<PluginId, ExternalPlugin>::new();
        let mut external_manifests = Vec::new();
        for candidate in PluginLoader::configured_candidates(&config.config_dir, &config.plugins) {
            if candidate.status != ConfiguredStatus::Eligible {
                continue;
            }
            let Ok(plugin) = ExternalPlugin::load(&candidate.path, ProcessLimits::default()).await else {
                continue;
            };
            if plugin.manifest().plugin_id.as_str() != candidate.name {
                continue;
            }
            let manifest = plugin.resolvable_manifest();
            let plugin_id = manifest.plugin_id.clone();
            external_manifests.push(CapabilityPlugin {
                manifest,
                origin: PluginOrigin::Configured {
                    executable: candidate.path.display().to_string(),
                    package: candidate.package,
                },
                authorized_replacements: candidate.replaces,
                configured: true,
            });
            external_plugins.insert(plugin_id, plugin);
        }

        let resolution =
            CapabilityResolver::resolve(vec![crate::plugin::builtin::capability_plugin()], external_manifests);
        let mut tools = BTreeMap::new();
        let mut policies: Vec<Arc<dyn PermissionCapability>> = Vec::new();
        for (target_id, active) in resolution.active {
            if target_id.kind() == CapabilityKind::Permission {
                if active.plugin_id.as_str() == "rho.builtin" {
                    continue;
                }
                if let Some(plugin) = external_plugins.get(&active.plugin_id)
                    && let Ok(policy) = plugin.permission(&active.id)
                {
                    policies.push(Arc::new(policy) as Arc<dyn PermissionCapability>);
                }
                continue;
            }
            if target_id.kind() != CapabilityKind::Tool {
                continue;
            }
            let capability: Arc<dyn ToolCapability> = if active.plugin_id.as_str() == "rho.builtin" {
                let Some(capability) = builtins.get(&active.id) else {
                    continue;
                };
                Arc::clone(capability)
            } else {
                let Some(plugin) = external_plugins.get(&active.plugin_id) else {
                    continue;
                };
                let Ok(capability) = plugin.tool(&active.id) else {
                    continue;
                };
                Arc::new(capability)
            };
            tools.insert(
                target_id.name().to_string(),
                ActiveTool {
                    target_id,
                    descriptor: capability.descriptor(),
                    capability,
                },
            );
        }
        Ok(Self {
            tools,
            floor: Arc::new(floor(config, base_dir)?),
            policies: Arc::new(PolicyEvaluator::spawn(
                policies,
                PolicyFailureMode::Deny,
                PolicyLimits::default(),
            )),
        })
    }

    pub fn definitions(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|tool| tool.descriptor.clone()).collect()
    }

    pub fn provider_definitions(&self) -> Vec<crate::plugin::contract::ProviderToolDefinition> {
        self.tools
            .iter()
            .map(|(name, tool)| crate::plugin::contract::ProviderToolDefinition {
                id: format!("tool:{name}").parse().unwrap(),
                description: tool.descriptor.description.clone(),
                argument_schema: tool.descriptor.argument_schema.clone(),
            })
            .collect()
    }

    pub fn neutral_executor(self: &Arc<Self>, context: ToolContext) -> NeutralActiveToolExecutor {
        NeutralActiveToolExecutor {
            tools: Arc::clone(self),
            context: tokio::sync::Mutex::new(context),
        }
    }

    pub fn into_rig_tools(self) -> Vec<DynamicTool> {
        self.tools
            .into_iter()
            .map(|(name, tool)| {
                let description = tool.descriptor.description.clone();
                let mut schema = tool.descriptor.argument_schema.clone();
                crate::tools::types::normalize_schema(&mut schema);
                let floor = Arc::clone(&self.floor);
                let policies = Arc::clone(&self.policies);
                DynamicTool::new(name, description, schema, move |context, arguments| {
                    let floor = Arc::clone(&floor);
                    let policies = Arc::clone(&policies);
                    let tool = tool.clone();
                    Box::pin(async move {
                        tool.dispatch(
                            DispatchContext {
                                floor: &floor,
                                policies: &policies,
                                tool: context,
                            },
                            arguments,
                        )
                        .await
                    })
                })
            })
            .collect()
    }
}

fn floor(config: &Config, base_dir: &Path) -> Result<SafetyFloor> {
    let workspace = crate::tools::workspace::Workspace::with_exclusions(
        base_dir,
        [config.config_dir.clone(), config.sessions_dir.clone()],
    );
    Ok(SafetyFloor::new(
        workspace,
        HttpClient::new(config.allow_private_network)?,
    ))
}

/// A pending approval prompt paired with its dispatched call so events,
/// grants, and denials stay on the same call identity.
struct ApprovalPrompt<'a> {
    call: &'a DispatchedCall<'a>,
    request: ApprovalRequest,
}

impl ActiveTool {
    /// Host dispatch boundary: the host floor runs before any permission
    /// evaluation or invocation, for built-in and external tools alike.
    async fn dispatch(
        &self,
        mut runtime: DispatchContext<'_>,
        arguments: serde_json::Value,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        let internal_call_id = uuid::Uuid::new_v4().to_string();
        let call = DispatchedCall {
            internal_call_id: &internal_call_id,
            tool_name: self.target_id.name(),
            arguments: &arguments,
        };
        emit_call_classified(runtime.tool, &call);
        self.run_authorized(&mut runtime, &call).await
    }

    /// Host floor, then composed permission evaluation, then invocation. The
    /// terminal finish event always runs, even when an earlier stage refuses.
    async fn run_authorized(
        &self,
        runtime: &mut DispatchContext<'_>,
        call: &DispatchedCall<'_>,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        let outcome = self.authorize_and_invoke(runtime, call).await;
        emit_call_finished(runtime.tool, call, &outcome);
        outcome
    }

    async fn authorize_and_invoke(
        &self,
        runtime: &mut DispatchContext<'_>,
        call: &DispatchedCall<'_>,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        self.enforce_floor(runtime, call)?;
        let operation = self.permission_operation(runtime, call);
        match self.resolve_permission(runtime, &operation).await {
            Ok(PermissionDecision::Allow) => self.invoke(runtime, call).await,
            Ok(PermissionDecision::Deny { rationale }) => {
                emit_denied(runtime.tool, call);
                Err(ToolExecutionError::refused(rationale))
            }
            Ok(PermissionDecision::ApprovalRequired { rationale }) => {
                let prompt = ApprovalPrompt {
                    call,
                    request: approval_request(call, rationale),
                };
                self.approve(runtime, prompt).await
            }
            Err(error) => Err(error),
        }
    }

    /// Record an approval decision against the capability, then invoke on
    /// grant or refuse on denial.
    async fn approve(
        &self,
        runtime: &mut DispatchContext<'_>,
        prompt: ApprovalPrompt<'_>,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        let ApprovalPrompt { call, request } = prompt;
        let Some(capability) = runtime.tool.get::<ApprovalCapability>().cloned() else {
            return Err(ToolExecutionError::refused(MISSING_APPROVAL_MESSAGE));
        };
        match capability.request_approval_sink(request.clone()).await {
            ApprovalDecision::Denied { reason } => {
                capability.deny_once(request, reason.clone());
                emit_denied(runtime.tool, call);
                Err(ToolExecutionError::refused(format_denial(reason)))
            }
            ApprovalDecision::Approved => {
                capability.grant_once(call.tool_name, call.arguments);
                emit_granted(runtime.tool, call);
                self.invoke(runtime, call).await
            }
            ApprovalDecision::ApprovedForSession => {
                capability.grant_for_session(call.tool_name, call.arguments);
                emit_granted(runtime.tool, call);
                self.invoke(runtime, call).await
            }
        }
    }

    fn enforce_floor(
        &self,
        runtime: &DispatchContext<'_>,
        call: &DispatchedCall<'_>,
    ) -> std::result::Result<(), ToolExecutionError> {
        let effects = self.effective_effects();
        runtime
            .floor
            .enforce(FloorRequest {
                schema: &self.descriptor.argument_schema,
                effects: &effects,
                arguments: call.arguments,
            })
            .map_err(floor_error)
    }

    async fn resolve_permission(
        &self,
        runtime: &DispatchContext<'_>,
        operation: &RequestedOperation,
    ) -> std::result::Result<PermissionDecision, ToolExecutionError> {
        let class = ToolExecutionPolicy::classify(operation.tool_id.name(), &operation.arguments);
        let default_decision = match runtime.tool.get::<ApprovalCapability>() {
            Some(capability) => capability.decision(operation, &class),
            None => classification_decision(class),
        };
        runtime
            .policies
            .evaluate(PermissionRequest {
                operation: operation.clone(),
                default_decision,
            })
            .await
            .map_err(|error| ToolExecutionError::refused(error.to_string()))
    }

    async fn invoke(
        &self,
        runtime: &DispatchContext<'_>,
        call: &DispatchedCall<'_>,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        let host = RigToolHost(runtime.tool);
        let response = self
            .capability
            .invoke(
                &host,
                ToolInvocationRequest {
                    arguments: call.arguments.clone(),
                    context: invocation_context(runtime),
                },
            )
            .await
            .map_err(map_capability_error)?;
        if response.is_error {
            Err(ToolExecutionError::other(response.content))
        } else {
            Ok(ToolOutput::text(response.content))
        }
    }

    /// Declared effects for the operation: the active descriptor's effects
    /// unioned with the built-in declaration for a replaced tool, so a
    /// replacement cannot weaken the floor.
    fn permission_operation(&self, runtime: &DispatchContext<'_>, call: &DispatchedCall<'_>) -> RequestedOperation {
        let effects = self.effective_effects();
        RequestedOperation {
            tool_id: self.target_id.clone(),
            arguments: call.arguments.clone(),
            effects,
            context: invocation_context(runtime),
        }
    }

    fn effective_effects(&self) -> Vec<OperationEffect> {
        let mut effects: std::collections::BTreeSet<_> = self.descriptor.effects.iter().cloned().collect();
        if let Some(declaration) = DECLARATIONS
            .iter()
            .find(|declaration| declaration.name == self.target_id.name())
        {
            effects.extend(declaration.descriptor().effects);
        }
        effects.into_iter().collect()
    }
}

fn emit_call_classified(tool: &ToolContext, call: &DispatchedCall<'_>) {
    let Some(capability) = tool.get::<ApprovalCapability>() else {
        return;
    };
    capability.emit_sink(ToolEvent::CallClassified {
        internal_call_id: call.internal_call_id.to_string(),
        tool_name: call.tool_name.to_string(),
        arguments: call.arguments.clone(),
        class: ToolExecutionPolicy::classify(call.tool_name, call.arguments),
    });
}

fn emit_call_finished(
    tool: &ToolContext,
    call: &DispatchedCall<'_>,
    outcome: &std::result::Result<ToolOutput, ToolExecutionError>,
) {
    let Some(capability) = tool.get::<ApprovalCapability>() else {
        return;
    };
    let result = match outcome {
        Ok(output) => DispatchedResult {
            output: output.as_text().unwrap_or_default().to_string(),
            status: "success",
        },
        Err(error) => DispatchedResult {
            output: error.model_output().as_text().unwrap_or_default().to_string(),
            status: if error.is_refusal() { "denied" } else { "error" },
        },
    };
    emit_tool_finished(capability, *call, result);
}

fn emit_denied(tool: &ToolContext, call: &DispatchedCall<'_>) {
    emit_event(
        tool,
        ToolEvent::ApprovalDenied {
            internal_call_id: call.internal_call_id.to_string(),
            tool_name: call.tool_name.to_string(),
        },
    );
}

fn emit_granted(tool: &ToolContext, call: &DispatchedCall<'_>) {
    emit_event(
        tool,
        ToolEvent::ApprovalGranted {
            internal_call_id: call.internal_call_id.to_string(),
            tool_name: call.tool_name.to_string(),
        },
    );
}

/// Build the approval-prompt request from the host classification, upgrading
/// policy-required permissions on otherwise classified operations.
fn approval_request(call: &DispatchedCall<'_>, rationale: String) -> ApprovalRequest {
    let (tier, reasons) = match ToolExecutionPolicy::classify(call.tool_name, call.arguments) {
        crate::tools::policy::ExecutionClass::ApprovalRequired { tier, reasons } => (tier, reasons),
        crate::tools::policy::ExecutionClass::ReadOnly | crate::tools::policy::ExecutionClass::WorkspaceMutation => {
            (RiskTier::Mutating, vec![rationale])
        }
    };
    ApprovalRequest {
        tool_name: call.tool_name.to_string(),
        arguments: call.arguments.clone(),
        tier,
        reasons,
    }
}

fn emit_event(tool: &ToolContext, event: ToolEvent) {
    if let Some(capability) = tool.get::<ApprovalCapability>() {
        capability.emit_sink(event);
    }
}

const MISSING_APPROVAL_MESSAGE: &str = "Approval context is missing; no operation was executed";

/// Decision contributed by the host classification when no approval
/// capability is present in the tool context.
fn classification_decision(class: crate::tools::policy::ExecutionClass) -> PermissionDecision {
    match class {
        crate::tools::policy::ExecutionClass::ReadOnly | crate::tools::policy::ExecutionClass::WorkspaceMutation => {
            PermissionDecision::Allow
        }
        crate::tools::policy::ExecutionClass::ApprovalRequired { reasons, .. } => {
            PermissionDecision::ApprovalRequired {
                rationale: reasons.join("; "),
            }
        }
    }
}

fn invocation_context(runtime: &DispatchContext<'_>) -> InvocationContext {
    runtime
        .tool
        .get::<InvocationContext>()
        .cloned()
        .unwrap_or(InvocationContext {
            session_id: String::new(),
            working_directory: String::new(),
            has_interactive_ui: runtime.tool.get::<QuestionPort>().is_some(),
        })
}

fn floor_error(denial: FloorDenial) -> ToolExecutionError {
    match denial {
        FloorDenial::InvalidArguments(message) => ToolExecutionError::invalid_args(message),
        FloorDenial::Operation(message) => ToolExecutionError::permission_denied(message),
    }
}

pub struct NeutralActiveToolExecutor {
    tools: Arc<ActiveToolSet>,
    context: tokio::sync::Mutex<ToolContext>,
}

#[async_trait]
impl crate::engine::provider::host_loop::NeutralToolExecutor for NeutralActiveToolExecutor {
    async fn execute(
        &self,
        call: crate::engine::provider::host_loop::NeutralToolCall,
    ) -> std::result::Result<
        crate::engine::provider::host_loop::NeutralToolResult,
        crate::engine::provider::host_loop::NeutralTurnError,
    > {
        let crate::engine::provider::host_loop::NeutralToolCall {
            call_id,
            tool_id,
            arguments,
        } = call;
        let tool =
            self.tools.tools.get(tool_id.name()).ok_or_else(|| {
                crate::engine::provider::host_loop::NeutralTurnError::UnknownTool(tool_id.to_string())
            })?;
        let mut context = self.context.lock().await;
        // Dispatch owns the full lifecycle: classification events, host floor,
        // composed permission evaluation, approval prompting, and invocation.
        let result = tool
            .dispatch(
                DispatchContext {
                    floor: &self.tools.floor,
                    policies: &self.tools.policies,
                    tool: &mut context,
                },
                arguments,
            )
            .await;
        match result {
            Ok(output) => Ok(crate::engine::provider::host_loop::NeutralToolResult {
                content: output.as_text().unwrap_or_default().to_string(),
                is_error: false,
            }),
            Err(error) => Err(crate::engine::provider::host_loop::NeutralTurnError::Tool(format!(
                "{call_id}: {}",
                error.message()
            ))),
        }
    }
}

fn map_capability_error(error: CapabilityError) -> ToolExecutionError {
    match error {
        CapabilityError::InvalidRequest { message } => ToolExecutionError::invalid_args(message),
        CapabilityError::PermissionDenied { message } => ToolExecutionError::permission_denied(message),
        CapabilityError::Cancelled => ToolExecutionError::cancelled("tool operation cancelled"),
        CapabilityError::Unavailable { message } => ToolExecutionError::provider(message),
        CapabilityError::Failed { message } => ToolExecutionError::other(message),
    }
}

struct RigToolHost<'a>(&'a ToolContext);

#[async_trait]
impl ToolHost for RigToolHost<'_> {
    async fn interact(&self, request: InteractionRequest) -> std::result::Result<InteractionResponse, CapabilityError> {
        let port = self
            .0
            .get::<QuestionPort>()
            .ok_or_else(|| CapabilityError::Unavailable {
                message: "interactive question port is unavailable".to_string(),
            })?;
        let response = port
            .ask(UserQuestion {
                question: request.question,
                header: request.header,
                options: request
                    .options
                    .into_iter()
                    .map(|option| UserQuestionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
                allow_custom: request.allow_custom,
            })
            .await
            .map_err(|error| CapabilityError::Failed {
                message: error.to_string(),
            })?;
        Ok(match response {
            UserAnswer::Selected(index) => InteractionResponse::Selected(index),
            UserAnswer::Custom(value) => InteractionResponse::Custom(value),
            UserAnswer::Cancelled => InteractionResponse::Cancelled,
        })
    }

    fn stream_chunk(&self, chunk: &str) {
        if let Some(port) = self.0.get::<crate::ui::ToolStreamPort>() {
            port.stream_chunk(chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::contract::{NetworkAccess, PathScope, ToolDescriptor, ToolInvocationResponse};
    use crate::tools::approval::{
        ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, ToolEvent, approval_context,
    };
    use crate::tools::ask_user::{InteractiveQuestionPort, UserAnswer, UserQuestion};
    use rig::tool::{ToolErrorKind, ToolSet};

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
        async fn ask(&self, _question: UserQuestion) -> std::result::Result<UserAnswer, crate::error::AppError> {
            Ok(UserAnswer::Custom("host answer".to_string()))
        }
    }

    struct DenyPolicy(Arc<std::sync::atomic::AtomicBool>);

    #[async_trait]
    impl PermissionCapability for DenyPolicy {
        fn id(&self) -> CapabilityId {
            "permission:deny-fixture".parse().unwrap()
        }

        async fn evaluate(
            &self,
            _request: RequestedOperation,
        ) -> std::result::Result<PermissionDecision, CapabilityError> {
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

        async fn evaluate(
            &self,
            _request: RequestedOperation,
        ) -> std::result::Result<PermissionDecision, CapabilityError> {
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

    fn fixture_descriptor(id: &str, effects: Vec<OperationEffect>) -> ToolDescriptor {
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
        }
    }

    fn fixture_tool(
        id: &str,
        effects: Vec<OperationEffect>,
        executed: &Arc<std::sync::atomic::AtomicBool>,
    ) -> ActiveTool {
        let descriptor = fixture_descriptor(id, effects);
        ActiveTool {
            target_id: descriptor.id.clone(),
            descriptor: descriptor.clone(),
            capability: Arc::new(FixtureTool(descriptor, Arc::clone(executed))),
        }
    }

    struct FixtureTool(ToolDescriptor, Arc<std::sync::atomic::AtomicBool>);

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

    /// A tool fixture plus configured restrictive policies for one test set.
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
            floor: Arc::new(floor(config, base_dir).unwrap()),
            policies: Arc::clone(&policies),
        };
        (ToolSet::from_dynamic_tools(active.into_rig_tools()), policies)
    }

    #[tokio::test]
    async fn builtin_interaction_routes_through_the_host_context() {
        let config = Config::default();
        let active = ActiveToolSet::builtins(&config, &std::env::temp_dir()).unwrap();
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
            DispatchFixture::tool(fixture_tool("read", Vec::new(), &executed)).with_policy(Arc::new(DenyPolicy(
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ))),
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
}
