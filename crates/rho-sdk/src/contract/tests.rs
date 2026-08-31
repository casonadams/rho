use super::*;
use crate::capability::CapabilityError;
use async_trait::async_trait;
use futures::stream::{self, BoxStream};

fn id(value: &str) -> CapabilityId {
    value.parse().unwrap()
}

struct Fixture;

#[async_trait]
impl ProviderCapability for Fixture {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: id("provider:fixture"),
            display_name: "Fixture".to_string(),
            models: Vec::new(),
            authentication: vec![AuthenticationMethod::None],
        }
    }

    async fn authenticate(&self, _request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        Ok(AuthenticationResponse {
            authenticated: true,
            refreshed_credential: None,
            user_message: None,
        })
    }

    async fn stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        Ok(Box::pin(stream::iter([Ok(ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        })])))
    }
}

#[async_trait]
impl ToolCapability for Fixture {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: id("tool:fixture"),
            description: "Fixture".to_string(),
            argument_schema: serde_json::json!({"type": "object"}),
            prompt_guidance: String::new(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Parallel,
        }
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        _request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        Ok(ToolInvocationResponse {
            content: "ok".to_string(),
            is_error: false,
            structured_content: None,
        })
    }
}

#[async_trait]
impl PermissionCapability for Fixture {
    fn id(&self) -> CapabilityId {
        id("permission:fixture")
    }

    async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
        Ok(PermissionDecision::Allow)
    }
}

#[async_trait]
impl CommandCapability for Fixture {
    fn descriptor(&self) -> CommandDescriptor {
        CommandDescriptor {
            id: id("command:fixture"),
            name: "fixture".to_string(),
            description: String::new(),
        }
    }

    async fn invoke(&self, _request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError> {
        Ok(CommandInvocationResponse {
            output: String::new(),
            exit_code: 0,
        })
    }
}

#[async_trait]
impl LifecycleCapability for Fixture {
    fn id(&self) -> CapabilityId {
        id("lifecycle:fixture")
    }

    async fn notify(&self, _event: LifecycleEvent) -> Result<(), CapabilityError> {
        Ok(())
    }
}

#[async_trait]
impl SkillCapability for Fixture {
    fn id(&self) -> CapabilityId {
        id("skill:fixture")
    }

    async fn assets(&self) -> Result<Vec<SkillAsset>, CapabilityError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ContextCapability for Fixture {
    fn descriptor(&self) -> ContextDescriptor {
        ContextDescriptor {
            id: id("context:fixture"),
            display_name: "Fixture context".to_string(),
            description: "Fixture context retrieval".to_string(),
            max_snippets: Some(5),
        }
    }

    async fn retrieve(&self, _request: ContextRequest) -> Result<ContextResponse, CapabilityError> {
        Ok(ContextResponse {
            snippets: vec![ContextSnippet {
                source: "fixture.md".to_string(),
                title: Some("Fixture Title".to_string()),
                content: "Fixture content".to_string(),
                score: Some(0.95),
            }],
        })
    }
}

fn assert_contracts<T>()
where
    T: ProviderCapability
        + ToolCapability
        + PermissionCapability
        + CommandCapability
        + LifecycleCapability
        + SkillCapability
        + ContextCapability,
{
}

#[test]
fn fixture_implements_every_framework_neutral_contract() {
    assert_contracts::<Fixture>();
}

#[test]
fn validates_contract_kinds_decisions_and_normalized_operations() {
    use crate::capability::CapabilityValidationError;

    let descriptor = ToolDescriptor {
        id: id("provider:wrong"),
        description: "tool".to_string(),
        argument_schema: serde_json::json!({}),
        prompt_guidance: String::new(),
        effects: Vec::new(),
        execution_mode: ExecutionMode::Sequential,
    };
    assert!(matches!(
        descriptor.validate(),
        Err(ContractValidationError::Capability(
            CapabilityValidationError::UnexpectedCapabilityKind { .. }
        ))
    ));
    assert_eq!(
        PermissionDecision::Deny {
            rationale: " ".to_string()
        }
        .validate(),
        Err(ContractValidationError::EmptyField("permission rationale"))
    );

    let operation = RequestedOperation {
        tool_id: id("tool:fixture"),
        arguments: serde_json::json!({}),
        effects: vec![OperationEffect::UserInteraction, OperationEffect::ExecuteProcess],
        context: InvocationContext {
            session_id: "session".to_string(),
            working_directory: "/workspace".to_string(),
            has_interactive_ui: true,
            plugin_config: None,
        },
    };
    assert_eq!(operation.validate(), Err(ContractValidationError::EffectsNotNormalized));
    operation.normalize().unwrap().validate().unwrap();
}

#[test]
fn credential_debug_output_is_redacted() {
    let secret = "credential-value";
    let credential = ScopedCredential {
        kind: "oauth".to_string(),
        value: serde_json::json!({"access_token": secret}),
    };
    let request = ProviderRequest {
        model: "fixture".to_string(),
        messages: Vec::new(),
        credential: Some(credential),
        max_output_tokens: None,
        tools: Vec::new(),
    };
    let output = format!("{request:?}");
    assert!(!output.contains(secret));
    assert!(output.contains("[REDACTED]"));
}

#[test]
fn contract_types_have_stable_tagged_serialization() {
    assert_eq!(
        serde_json::to_value(ExecutionMode::Parallel).unwrap(),
        serde_json::json!("parallel")
    );
    assert_eq!(
        serde_json::to_value(ExecutionMode::Sequential).unwrap(),
        serde_json::json!("sequential")
    );
    assert_eq!(
        serde_json::from_str::<ExecutionMode>("\"parallel\"").unwrap(),
        ExecutionMode::Parallel
    );
    assert_eq!(
        serde_json::from_str::<ExecutionMode>("\"sequential\"").unwrap(),
        ExecutionMode::Sequential
    );
    assert_eq!(ExecutionMode::default(), ExecutionMode::Sequential);

    let decision = PermissionDecision::ApprovalRequired {
        rationale: "confirm".to_string(),
    };
    assert_eq!(
        serde_json::to_value(decision).unwrap(),
        serde_json::json!({"decision": "approval_required", "rationale": "confirm"})
    );
    let event = ProviderStreamEvent::TextDelta {
        text: "hello".to_string(),
    };
    let encoded = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<ProviderStreamEvent>(&encoded).unwrap(), event);

    let context_desc = ContextDescriptor {
        id: id("context:fixture"),
        display_name: "Fixture Context".to_string(),
        description: "Retrieval fixture".to_string(),
        max_snippets: Some(10),
    };
    context_desc.validate().unwrap();

    let lifecycle_before = LifecycleEvent::BeforeTurn {
        session_id: "s1".to_string(),
        prompt: "what is rho".to_string(),
        working_directory: "/app".to_string(),
    };
    let encoded = serde_json::to_string(&lifecycle_before).unwrap();
    assert_eq!(
        serde_json::from_str::<LifecycleEvent>(&encoded).unwrap(),
        lifecycle_before
    );

    let lifecycle_after = LifecycleEvent::AfterTurn {
        session_id: "s1".to_string(),
        success: true,
        files_modified: vec!["src/main.rs".to_string()],
    };
    let encoded = serde_json::to_string(&lifecycle_after).unwrap();
    assert_eq!(
        serde_json::from_str::<LifecycleEvent>(&encoded).unwrap(),
        lifecycle_after
    );
}
