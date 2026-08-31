use crate::safety_floor::{FloorDenial, SafetyFloor};
use async_trait::async_trait;
use rho_core::approval::{
    ApprovalCapability, ApprovalDecision, ApprovalRequest, DispatchedCall, DispatchedResult, ToolEvent,
    emit_tool_finished, format_denial,
};
use rho_core::bash_ast::RiskTier;
use rho_core::policy::ToolExecutionPolicy;
use rho_core::presentation::questions::{QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
use rho_plugin_builtin::DECLARATIONS;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{
    InteractionRequest, InteractionResponse, InvocationContext, OperationEffect, PermissionDecision,
    RequestedOperation, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
};
use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use std::sync::Arc;

pub(crate) const MISSING_APPROVAL_MESSAGE: &str = "Approval context is missing; no operation was executed";

#[derive(Clone)]
pub struct ActiveTool {
    pub(crate) target_id: CapabilityId,
    pub(crate) descriptor: ToolDescriptor,
    pub(crate) capability: Arc<dyn ToolCapability>,
}

pub struct DispatchContext<'a> {
    pub(crate) floor: &'a SafetyFloor,
    pub(crate) policies: &'a crate::permission::PolicyEvaluator,
    pub(crate) tool: &'a mut ToolContext,
}

pub struct ApprovalPrompt<'a> {
    pub(crate) call: &'a DispatchedCall<'a>,
    pub(crate) request: ApprovalRequest,
}

impl ActiveTool {
    pub async fn dispatch(
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
            .enforce(crate::safety_floor::FloorRequest {
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
            .evaluate(crate::permission::PermissionRequest {
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

pub fn emit_call_classified(tool: &ToolContext, call: &DispatchedCall<'_>) {
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

pub fn emit_call_finished(
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

pub fn emit_denied(tool: &ToolContext, call: &DispatchedCall<'_>) {
    emit_event(
        tool,
        ToolEvent::ApprovalDenied {
            internal_call_id: call.internal_call_id.to_string(),
            tool_name: call.tool_name.to_string(),
        },
    );
}

pub fn emit_granted(tool: &ToolContext, call: &DispatchedCall<'_>) {
    emit_event(
        tool,
        ToolEvent::ApprovalGranted {
            internal_call_id: call.internal_call_id.to_string(),
            tool_name: call.tool_name.to_string(),
        },
    );
}

pub fn approval_request(call: &DispatchedCall<'_>, rationale: String) -> ApprovalRequest {
    let (tier, reasons) = match ToolExecutionPolicy::classify(call.tool_name, call.arguments) {
        rho_core::policy::ExecutionClass::ApprovalRequired { tier, reasons } => (tier, reasons),
        rho_core::policy::ExecutionClass::ReadOnly | rho_core::policy::ExecutionClass::WorkspaceMutation => {
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

pub fn emit_event(tool: &ToolContext, event: ToolEvent) {
    if let Some(capability) = tool.get::<ApprovalCapability>() {
        capability.emit_sink(event);
    }
}

pub fn classification_decision(class: rho_core::policy::ExecutionClass) -> PermissionDecision {
    match class {
        rho_core::policy::ExecutionClass::ReadOnly | rho_core::policy::ExecutionClass::WorkspaceMutation => {
            PermissionDecision::Allow
        }
        rho_core::policy::ExecutionClass::ApprovalRequired { reasons, .. } => PermissionDecision::ApprovalRequired {
            rationale: reasons.join("; "),
        },
    }
}

pub fn invocation_context(runtime: &DispatchContext<'_>) -> InvocationContext {
    runtime
        .tool
        .get::<InvocationContext>()
        .cloned()
        .unwrap_or(InvocationContext {
            session_id: String::new(),
            working_directory: String::new(),
            has_interactive_ui: runtime.tool.get::<QuestionPort>().is_some(),
            plugin_config: None,
        })
}

pub fn floor_error(denial: FloorDenial) -> ToolExecutionError {
    match denial {
        FloorDenial::InvalidArguments(message) => ToolExecutionError::invalid_args(message),
        FloorDenial::Operation(message) => ToolExecutionError::permission_denied(message),
    }
}

pub fn map_capability_error(error: CapabilityError) -> ToolExecutionError {
    match error {
        CapabilityError::InvalidRequest { message } => ToolExecutionError::invalid_args(message),
        CapabilityError::PermissionDenied { message } => ToolExecutionError::permission_denied(message),
        CapabilityError::Cancelled => ToolExecutionError::cancelled("tool operation cancelled"),
        CapabilityError::Unavailable { message } => ToolExecutionError::provider(message),
        CapabilityError::Failed { message } => ToolExecutionError::other(message),
    }
}

pub struct RigToolHost<'a>(pub &'a ToolContext);

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
        if let Some(port) = self.0.get::<rho_core::presentation::stream::ToolStreamPort>() {
            port.stream_chunk(chunk);
        }
    }

    fn progress(&self, message: &str) {
        if let Some(port) = self.0.get::<rho_core::presentation::stream::ToolStreamPort>() {
            port.stream_chunk(message);
        }
    }
}
