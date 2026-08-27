#![cfg(unix)]

use rho::config::{Config, PluginConfig};
use rho::plugin::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho::plugin::contract::{CapabilityDescriptor, OperationEffect, ToolDescriptor, ToolInvocationResponse};
use rho::plugin::protocol::{ProtocolMessage, TerminalResult};
use rho::plugin::tool_dispatch::ActiveToolSet;
use rig::tool::{ToolContext, ToolSet};
use std::collections::BTreeSet;
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
    let capability_id: CapabilityId = "tool:replacement-bash".parse().unwrap();
    let manifest = CapabilityManifest {
        plugin_id: "bash-fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: capability_id.clone(),
            replaces: Some("tool:bash".parse().unwrap()),
        }],
    };
    let descriptor = CapabilityDescriptor::Tool(ToolDescriptor {
        id: capability_id,
        description: "Fixture bash replacement".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {"command": {"type": "string"}},
            "additionalProperties": false
        }),
        prompt_guidance: "Use the fixture replacement.".to_string(),
        effects: vec![OperationEffect::ExecuteProcess],
    });
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
            content: "replacement executed".to_string(),
            is_error: false,
            structured_content: None,
        }),
    }));
    let root = std::env::temp_dir().join(format!("rho_override_{}", uuid::Uuid::new_v4()));
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

fn config_for(fixture: &Fixture, replacements: BTreeSet<CapabilityId>) -> Config {
    let mut config = Config {
        config_dir: fixture.root.clone(),
        ..Config::default()
    };
    config.plugins.insert(
        "bash-fixture".to_string(),
        PluginConfig {
            path: fixture.executable.clone(),
            package: None,
            replaces: replacements,
        },
    );
    config
}

#[tokio::test]
async fn configured_global_plugin_replaces_model_facing_bash() {
    let fixture = fixture();
    let config = config_for(&fixture, BTreeSet::from(["tool:bash".parse().unwrap()]));
    let active = ActiveToolSet::load(&config, &fixture.root).await.unwrap();
    let definitions = active.definitions();
    let bash = definitions
        .iter()
        .find(|descriptor| descriptor.id == "tool:replacement-bash".parse().unwrap())
        .unwrap();
    assert_eq!(bash.description, "Fixture bash replacement");

    let tools = ToolSet::from_dynamic_tools(active.into_rig_tools());
    let result = tools
        .execute(
            "bash",
            r#"{"command":"printf builtin-must-not-run"}"#,
            &mut ToolContext::new(),
        )
        .await;
    assert_eq!(result.output().as_text(), Some("replacement executed"));
}

#[tokio::test]
async fn unauthorized_replacement_preserves_builtin_bash() {
    let fixture = fixture();
    let config = config_for(&fixture, BTreeSet::new());
    let active = ActiveToolSet::load(&config, &fixture.root).await.unwrap();
    let definitions = active.definitions();

    assert!(
        definitions
            .iter()
            .any(|descriptor| descriptor.id == "tool:bash".parse().unwrap())
    );
    assert!(
        !definitions
            .iter()
            .any(|descriptor| descriptor.id == "tool:replacement-bash".parse().unwrap())
    );
}
