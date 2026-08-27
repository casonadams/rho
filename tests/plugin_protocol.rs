#![cfg(unix)]

use rho::plugin::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho::plugin::contract::{
    CapabilityDescriptor, InvocationContext, ToolCapability, ToolDescriptor, ToolInvocationRequest,
    ToolInvocationResponse,
};
use rho::plugin::external::ExternalPlugin;
use rho::plugin::process::ProcessLimits;
use rho::plugin::protocol::{ProtocolMessage, TerminalResult};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fragment(message: ProtocolMessage) -> String {
    let encoded = serde_json::to_string(&message).unwrap();
    encoded[1..encoded.len() - 1].to_string()
}

fn response(fragment: &str) -> String {
    format!("printf '{{\"protocol_version\":1,\"request_id\":\"%s\",{fragment}}}\\n' \"$request_id\"\n")
}

fn fixture() -> Fixture {
    let tool_id: CapabilityId = "tool:fixture".parse().unwrap();
    let descriptor = CapabilityDescriptor::Tool(ToolDescriptor {
        id: tool_id.clone(),
        description: "Fixture tool".to_string(),
        argument_schema: serde_json::json!({"type": "object"}),
        prompt_guidance: String::new(),
        effects: Vec::new(),
    });
    let manifest = CapabilityManifest {
        plugin_id: "protocol-fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: tool_id,
            replaces: None,
        }],
    };
    let handshake = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Handshake {
            selected_version: PLUGIN_PROTOCOL_VERSION,
        },
    }));
    let discovery = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Discovery {
            manifest,
            capabilities: vec![descriptor],
        },
    }));
    let invocation = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Tool(ToolInvocationResponse {
            content: "fixture-result".to_string(),
            is_error: false,
            structured_content: None,
        }),
    }));
    let root = std::env::temp_dir().join(format!("rho_protocol_{}", uuid::Uuid::new_v4()));
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
  *\"kind\":\"tool\"*) {invocation} ;;
esac
"#
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fixture { root, executable }
}

#[tokio::test]
async fn discovers_and_invokes_a_subprocess_plugin_end_to_end() {
    let fixture = fixture();
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();
    let tool = plugin.tool(&"tool:fixture".parse().unwrap()).unwrap();
    let response = tool
        .invoke(ToolInvocationRequest {
            arguments: serde_json::json!({}),
            context: InvocationContext {
                session_id: "session".to_string(),
                working_directory: "/workspace".to_string(),
                has_interactive_ui: false,
            },
        })
        .await
        .unwrap();

    assert_eq!(response.content, "fixture-result");
}
