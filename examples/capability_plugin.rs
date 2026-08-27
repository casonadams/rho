use rho::plugin::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho::plugin::contract::{
    AuthenticationMethod, AuthenticationResponse, CapabilityDescriptor, CommandDescriptor, CommandInvocationResponse,
    FinishReason, ModelMetadata, PermissionDecision, ProviderDescriptor, ProviderStreamEvent, SkillAsset,
    ToolDescriptor, ToolInvocationResponse,
};
use rho::plugin::protocol::{
    Envelope, ErrorCode, InvocationRequest, ProtocolMessage, StreamEvent, StructuredError, TerminalResult,
};
use std::io::{BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let request = match rho::plugin::protocol::decode_line(line.as_bytes()) {
            Ok(request) => request,
            Err(_) => continue,
        };
        match request.message {
            ProtocolMessage::HandshakeRequest { supported_versions }
                if supported_versions.contains(&PLUGIN_PROTOCOL_VERSION) =>
            {
                terminal(
                    &mut stdout,
                    request.request_id,
                    TerminalResult::Handshake {
                        selected_version: PLUGIN_PROTOCOL_VERSION,
                    },
                )?;
            }
            ProtocolMessage::DiscoveryRequest => {
                let (manifest, capabilities) = discovery();
                terminal(
                    &mut stdout,
                    request.request_id,
                    TerminalResult::Discovery { manifest, capabilities },
                )?;
            }
            ProtocolMessage::InvocationRequest { invocation, .. } => {
                invoke(&mut stdout, request.request_id, invocation)?;
            }
            ProtocolMessage::CancelRequest { target_request_id } => {
                terminal(&mut stdout, target_request_id, TerminalResult::Cancelled)?;
                terminal(&mut stdout, request.request_id, TerminalResult::Cancelled)?;
            }
            _ => error(&mut stdout, request.request_id, ErrorCode::InvalidRequest)?,
        }
    }
    Ok(())
}

fn discovery() -> (CapabilityManifest, Vec<CapabilityDescriptor>) {
    let provider_id: CapabilityId = "provider:fixture".parse().unwrap();
    let tool_id: CapabilityId = "tool:fixture".parse().unwrap();
    let permission_id: CapabilityId = "permission:fixture".parse().unwrap();
    let command_id: CapabilityId = "command:fixture".parse().unwrap();
    let lifecycle_id: CapabilityId = "lifecycle:fixture".parse().unwrap();
    let skill_id: CapabilityId = "skill:fixture".parse().unwrap();
    let capabilities = vec![
        CapabilityDescriptor::Provider(ProviderDescriptor {
            id: provider_id.clone(),
            display_name: "Fixture provider".to_string(),
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
            id: tool_id.clone(),
            description: "Return fixture output".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {"message": {"type": "string"}},
                "required": ["message"],
                "additionalProperties": false
            }),
            prompt_guidance: "Use for protocol tests.".to_string(),
            effects: Vec::new(),
        }),
        CapabilityDescriptor::Permission {
            id: permission_id.clone(),
        },
        CapabilityDescriptor::Command(CommandDescriptor {
            id: command_id.clone(),
            name: "fixture".to_string(),
            description: "Run the fixture command".to_string(),
        }),
        CapabilityDescriptor::Lifecycle {
            id: lifecycle_id.clone(),
        },
        CapabilityDescriptor::Skill { id: skill_id.clone() },
    ];
    let manifest = CapabilityManifest {
        plugin_id: "fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: [provider_id, tool_id, permission_id, command_id, lifecycle_id, skill_id]
            .into_iter()
            .map(|id| CapabilityDeclaration { id, replaces: None })
            .collect(),
    };
    (manifest, capabilities)
}

fn invoke(
    stdout: &mut impl Write,
    request_id: rho::plugin::protocol::RequestId,
    invocation: InvocationRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    match invocation {
        InvocationRequest::ProviderStream(_) => {
            event(
                stdout,
                request_id.clone(),
                StreamEvent::Provider(ProviderStreamEvent::TextDelta {
                    text: "fixture response".to_string(),
                }),
            )?;
            event(
                stdout,
                request_id.clone(),
                StreamEvent::Provider(ProviderStreamEvent::Finished {
                    reason: FinishReason::Stop,
                }),
            )?;
            terminal(stdout, request_id, TerminalResult::StreamCompleted)?;
        }
        InvocationRequest::ProviderAuthenticate(_) => terminal(
            stdout,
            request_id,
            TerminalResult::ProviderAuthenticated(AuthenticationResponse {
                authenticated: true,
                refreshed_credential: None,
                user_message: None,
            }),
        )?,
        InvocationRequest::Tool(request) => terminal(
            stdout,
            request_id,
            TerminalResult::Tool(ToolInvocationResponse {
                content: request.arguments["message"].as_str().unwrap_or_default().to_string(),
                is_error: false,
                structured_content: None,
            }),
        )?,
        InvocationRequest::Permission(_) => terminal(
            stdout,
            request_id,
            TerminalResult::Permission(PermissionDecision::Allow),
        )?,
        InvocationRequest::Command(request) => terminal(
            stdout,
            request_id,
            TerminalResult::Command(CommandInvocationResponse {
                output: request.arguments.join(" "),
                exit_code: 0,
            }),
        )?,
        InvocationRequest::Lifecycle(_) => terminal(stdout, request_id, TerminalResult::Lifecycle)?,
        InvocationRequest::Skills => terminal(
            stdout,
            request_id,
            TerminalResult::Skills(vec![SkillAsset {
                id: "skill:fixture".parse().unwrap(),
                name: "fixture".to_string(),
                description: "Fixture skill".to_string(),
                markdown: "# Fixture".to_string(),
            }]),
        )?,
    }
    Ok(())
}

fn event(
    stdout: &mut impl Write,
    request_id: rho::plugin::protocol::RequestId,
    event: StreamEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    write_message(
        stdout,
        Envelope::new(request_id, ProtocolMessage::StreamEvent { event }),
    )
}

fn terminal(
    stdout: &mut impl Write,
    request_id: rho::plugin::protocol::RequestId,
    result: TerminalResult,
) -> Result<(), Box<dyn std::error::Error>> {
    write_message(
        stdout,
        Envelope::new(request_id, ProtocolMessage::TerminalResponse { result }),
    )
}

fn error(
    stdout: &mut impl Write,
    request_id: rho::plugin::protocol::RequestId,
    code: ErrorCode,
) -> Result<(), Box<dyn std::error::Error>> {
    write_message(
        stdout,
        Envelope::new(
            request_id,
            ProtocolMessage::ErrorResponse {
                error: StructuredError::public(code, "invalid request", false),
            },
        ),
    )
}

fn write_message(stdout: &mut impl Write, envelope: Envelope) -> Result<(), Box<dyn std::error::Error>> {
    stdout.write_all(&rho::plugin::protocol::encode_line(&envelope)?)?;
    stdout.flush()?;
    Ok(())
}
