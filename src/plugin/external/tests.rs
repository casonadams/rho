use super::*;
use crate::plugin::capability::{CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest};
use crate::plugin::contract::{
    AuthenticationMethod, AuthenticationOperation, FinishReason, InvocationContext, LifecycleEvent, ModelMetadata,
    PermissionDecision, ProviderStreamEvent,
};
use crate::plugin::protocol::{ProtocolMessage, TerminalResult};
use futures::StreamExt;
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn message_fragment(message: ProtocolMessage) -> String {
    let encoded = serde_json::to_string(&message).unwrap();
    encoded[1..encoded.len() - 1].to_string()
}

fn emit(fragment: &str) -> String {
    format!("printf '{{\"protocol_version\":1,\"request_id\":\"%s\",{fragment}}}\\n' \"$request_id\"\n")
}

fn fixture(capabilities: Vec<CapabilityDescriptor>) -> Fixture {
    let declarations = capabilities
        .iter()
        .map(|descriptor| CapabilityDeclaration {
            id: descriptor.id().clone(),
            replaces: None,
        })
        .collect();
    let manifest = CapabilityManifest {
        plugin_id: "external-fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: crate::plugin::capability::PLUGIN_PROTOCOL_VERSION,
        capabilities: declarations,
    };
    let discovery = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Discovery { manifest, capabilities },
    }));
    let handshake = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Handshake {
            selected_version: crate::plugin::capability::PLUGIN_PROTOCOL_VERSION,
        },
    }));
    let provider_event = emit(&message_fragment(ProtocolMessage::StreamEvent {
        event: StreamEvent::Provider(ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        }),
    }));
    let provider_terminal = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::StreamCompleted,
    }));
    let auth = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::ProviderAuthenticated(AuthenticationResponse {
            authenticated: true,
            refreshed_credential: None,
            user_message: None,
        }),
    }));
    let tool = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Tool(ToolInvocationResponse {
            content: "tool-ok".to_string(),
            is_error: false,
            structured_content: None,
        }),
    }));
    let permission = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Permission(PermissionDecision::Allow),
    }));
    let command = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Command(CommandInvocationResponse {
            output: "command-ok".to_string(),
            exit_code: 0,
        }),
    }));
    let lifecycle = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Lifecycle,
    }));
    let skills = emit(&message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Skills(vec![SkillAsset {
            id: "skill:fixture".parse().unwrap(),
            name: "Fixture".to_string(),
            description: "Fixture skill".to_string(),
            markdown: "# Fixture".to_string(),
        }]),
    }));
    let root = std::env::temp_dir().join(format!("rho_external_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executable = root.join("plugin");
    let script = format!(
        r#"#!/bin/sh
read handshake
request_id=$(printf '%s' "$handshake" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
{handshake}read request
request_id=$(printf '%s' "$request" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
case "$request" in
  *\"type\":\"discovery_request\"*) {discovery} ;;
  *\"kind\":\"provider_stream\"*) {provider_event}{provider_terminal} ;;
  *\"kind\":\"provider_authenticate\"*) {auth} ;;
  *\"kind\":\"tool\"*) {tool} ;;
  *\"kind\":\"permission\"*) {permission} ;;
  *\"kind\":\"command\"*) {command} ;;
  *\"kind\":\"lifecycle\"*) {lifecycle} ;;
  *\"kind\":\"skills\"*) {skills} ;;
esac
"#
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fixture { root, executable }
}

fn descriptors() -> Vec<CapabilityDescriptor> {
    vec![
        CapabilityDescriptor::Provider(ProviderDescriptor {
            id: "provider:fixture".parse().unwrap(),
            display_name: "Fixture".to_string(),
            models: vec![ModelMetadata {
                id: "fixture-model".to_string(),
                display_name: "Fixture model".to_string(),
                context_limit: Some(4096),
                supports_tools: true,
                supports_images: false,
            }],
            authentication: vec![AuthenticationMethod::None],
        }),
        CapabilityDescriptor::Tool(ToolDescriptor {
            id: "tool:fixture".parse().unwrap(),
            description: "Fixture tool".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["message"],
                "properties": {"message": {"type": "string"}},
                "additionalProperties": false
            }),
            prompt_guidance: String::new(),
            effects: Vec::new(),
        }),
        CapabilityDescriptor::Permission {
            id: "permission:fixture".parse().unwrap(),
        },
        CapabilityDescriptor::Command(CommandDescriptor {
            id: "command:fixture".parse().unwrap(),
            name: "fixture".to_string(),
            description: "Fixture command".to_string(),
        }),
        CapabilityDescriptor::Lifecycle {
            id: "lifecycle:fixture".parse().unwrap(),
        },
        CapabilityDescriptor::Skill {
            id: "skill:fixture".parse().unwrap(),
        },
    ]
}

fn context() -> InvocationContext {
    InvocationContext {
        session_id: "session".to_string(),
        working_directory: "/workspace".to_string(),
        has_interactive_ui: false,
    }
}

#[tokio::test]
async fn invokes_every_external_capability_contract() {
    let fixture = fixture(descriptors());
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();

    let provider_id = "provider:fixture".parse().unwrap();
    let provider = plugin.provider(&provider_id).unwrap();
    let authenticated = provider
        .authenticate(AuthenticationRequest {
            operation: AuthenticationOperation::Verify,
            credential: None,
        })
        .await
        .unwrap();
    assert!(authenticated.authenticated);
    let events = provider
        .stream(ProviderRequest {
            model: "fixture-model".to_string(),
            messages: Vec::new(),
            credential: None,
            max_output_tokens: None,
            tools: Vec::new(),
        })
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events,
        vec![Ok(ProviderStreamEvent::Finished {
            reason: FinishReason::Stop
        })]
    );

    let tool_id = "tool:fixture".parse().unwrap();
    let tool = plugin.tool(&tool_id).unwrap();
    let response = tool
        .invoke(ToolInvocationRequest {
            arguments: serde_json::json!({"message": "hello"}),
            context: context(),
        })
        .await
        .unwrap();
    assert_eq!(response.content, "tool-ok");
    assert!(matches!(
        tool.invoke(ToolInvocationRequest {
            arguments: serde_json::json!({}),
            context: context(),
        })
        .await,
        Err(CapabilityError::InvalidRequest { .. })
    ));

    let permission = plugin.permission(&"permission:fixture".parse().unwrap()).unwrap();
    assert_eq!(
        permission
            .evaluate(RequestedOperation {
                tool_id,
                arguments: serde_json::json!({}),
                effects: Vec::new(),
                context: context(),
            })
            .await
            .unwrap(),
        PermissionDecision::Allow
    );

    let command = plugin.command(&"command:fixture".parse().unwrap()).unwrap();
    assert_eq!(
        command
            .invoke(CommandInvocationRequest {
                arguments: vec!["argument".to_string()],
                context: context(),
            })
            .await
            .unwrap()
            .output,
        "command-ok"
    );

    plugin
        .lifecycle(&"lifecycle:fixture".parse().unwrap())
        .unwrap()
        .notify(LifecycleEvent::HostStarted)
        .await
        .unwrap();
    let assets = plugin
        .skill(&"skill:fixture".parse().unwrap())
        .unwrap()
        .assets()
        .await
        .unwrap();
    assert_eq!(assets[0].name, "Fixture");
}

#[tokio::test]
async fn invalid_descriptor_disables_only_that_capability() {
    let mut capabilities = descriptors();
    let CapabilityDescriptor::Command(command) = &mut capabilities[3] else {
        panic!("command fixture missing");
    };
    command.description.clear();
    let fixture = fixture(capabilities);
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();

    assert!(plugin.command(&"command:fixture".parse().unwrap()).is_err());
    let tool = plugin.tool(&"tool:fixture".parse().unwrap()).unwrap();
    assert_eq!(
        tool.invoke(ToolInvocationRequest {
            arguments: serde_json::json!({"message": "hello"}),
            context: context(),
        })
        .await
        .unwrap()
        .content,
        "tool-ok"
    );
}
