use super::*;
use crate::capability::{CapabilityError, CapabilityId, PLUGIN_PROTOCOL_VERSION};
use crate::contract::{
    CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse, ContextCapability,
    ContextDescriptor, ContextRequest, ContextResponse, ContextSnippet,
};
use crate::protocol::{Envelope, InvocationRequest, ProtocolMessage, RequestId, TerminalResult, decode_line};
use async_trait::async_trait;
use std::io::Cursor;

struct TestCommand;

#[async_trait]
impl CommandCapability for TestCommand {
    fn descriptor(&self) -> CommandDescriptor {
        CommandDescriptor {
            id: "command:test".parse().unwrap(),
            name: "test".to_string(),
            description: "Test command".to_string(),
        }
    }

    async fn invoke(&self, request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError> {
        Ok(CommandInvocationResponse {
            output: format!("ran test with args: {}", request.arguments.join(" ")),
            exit_code: 0,
        })
    }
}

struct TestContext;

#[async_trait]
impl ContextCapability for TestContext {
    fn descriptor(&self) -> ContextDescriptor {
        ContextDescriptor {
            id: "context:test".parse().unwrap(),
            display_name: "Test Context".to_string(),
            description: "Provides test snippets".to_string(),
            max_snippets: Some(3),
        }
    }

    async fn retrieve(&self, request: ContextRequest) -> Result<ContextResponse, CapabilityError> {
        Ok(ContextResponse {
            snippets: vec![ContextSnippet {
                source: "test_doc.md".to_string(),
                title: Some("Title".to_string()),
                content: format!("Content for prompt: {}", request.prompt),
                score: Some(0.95),
            }],
        })
    }
}

#[tokio::test]
async fn test_plugin_builder_and_serve_handshake_discovery_invocation() {
    let plugin = PluginBuilder::new("test-plugin", "1.0.0")
        .command(TestCommand)
        .context(TestContext)
        .build()
        .unwrap();

    let handshake_req = Envelope::new(
        RequestId::new("req-1").unwrap(),
        ProtocolMessage::HandshakeRequest {
            supported_versions: vec![PLUGIN_PROTOCOL_VERSION],
            plugin_config: None,
        },
    );
    let discovery_req = Envelope::new(RequestId::new("req-2").unwrap(), ProtocolMessage::DiscoveryRequest);
    let command_cap_id: CapabilityId = "command:test".parse().unwrap();
    let invoke_req = Envelope::new(
        RequestId::new("req-3").unwrap(),
        ProtocolMessage::InvocationRequest {
            capability_id: command_cap_id,
            invocation: InvocationRequest::Command(CommandInvocationRequest {
                arguments: vec!["foo".to_string(), "bar".to_string()],
                context: crate::contract::InvocationContext {
                    session_id: "s1".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            }),
        },
    );

    let mut input_data = Vec::new();
    input_data.extend(serde_json::to_vec(&handshake_req).unwrap());
    input_data.push(b'\n');
    input_data.extend(serde_json::to_vec(&discovery_req).unwrap());
    input_data.push(b'\n');
    input_data.extend(serde_json::to_vec(&invoke_req).unwrap());
    input_data.push(b'\n');

    let mut output_buf = Vec::new();
    let mut cursor = Cursor::new(input_data);
    serve(&plugin, &mut cursor, &mut output_buf).await.unwrap();

    let output_str = String::from_utf8(output_buf).unwrap();
    let lines: Vec<&str> = output_str.lines().collect();
    assert_eq!(lines.len(), 3);

    let resp1: Envelope = decode_line(lines[0].as_bytes()).unwrap();
    assert_eq!(resp1.request_id.as_str(), "req-1");
    assert!(matches!(
        resp1.message,
        ProtocolMessage::TerminalResponse {
            result: TerminalResult::Handshake { selected_version: 1 }
        }
    ));

    let resp2: Envelope = decode_line(lines[1].as_bytes()).unwrap();
    assert_eq!(resp2.request_id.as_str(), "req-2");
    assert!(matches!(resp2.message, ProtocolMessage::TerminalResponse {
        result: TerminalResult::Discovery { ref manifest, ref capabilities }
    } if manifest.plugin_id.as_str() == "test-plugin" && capabilities.len() == 2));

    let resp3: Envelope = decode_line(lines[2].as_bytes()).unwrap();
    assert_eq!(resp3.request_id.as_str(), "req-3");
    assert!(matches!(resp3.message, ProtocolMessage::TerminalResponse {
        result: TerminalResult::Command(ref res)
    } if res.output.contains("ran test with args: foo bar")));
}
