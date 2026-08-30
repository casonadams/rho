#![cfg(unix)]

use async_trait::async_trait;
use rho::plugin::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho::plugin::contract::{
    CapabilityDescriptor, CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse,
    ContextCapability, ContextDescriptor, ContextRequest, ContextResponse, ContextSnippet, InteractionRequest,
    InteractionResponse, InvocationContext, LifecycleCapability, LifecycleEvent, ToolCapability, ToolDescriptor,
    ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use rho::plugin::external::ExternalPlugin;
use rho::plugin::process::ProcessLimits;
use rho::plugin::protocol::{ProtocolMessage, StreamEvent, TerminalResult};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

struct CapturingHost(Mutex<Vec<String>>);

#[async_trait]
impl ToolHost for CapturingHost {
    async fn interact(
        &self,
        _request: InteractionRequest,
    ) -> Result<InteractionResponse, rho::plugin::capability::CapabilityError> {
        Err(rho::plugin::capability::CapabilityError::Unavailable {
            message: "interaction unavailable".to_string(),
        })
    }

    fn stream_chunk(&self, chunk: &str) {
        self.0.lock().unwrap().push(chunk.to_string());
    }
}

struct KilnPluginFixture {
    root: PathBuf,
    executable: PathBuf,
}

impl Drop for KilnPluginFixture {
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

fn fixture() -> KilnPluginFixture {
    let context_id: CapabilityId = "context:kiln".parse().unwrap();
    let command_id: CapabilityId = "command:kiln".parse().unwrap();
    let lifecycle_id: CapabilityId = "lifecycle:kiln".parse().unwrap();
    let tool_id: CapabilityId = "tool:kiln_search".parse().unwrap();

    let capabilities = vec![
        CapabilityDescriptor::Context(ContextDescriptor {
            id: context_id.clone(),
            display_name: "Kiln Memory".to_string(),
            description: "Local RAG vector search".to_string(),
            max_snippets: Some(5),
        }),
        CapabilityDescriptor::Command(CommandDescriptor {
            id: command_id.clone(),
            name: "kiln".to_string(),
            description: "Manage local context bundles".to_string(),
        }),
        CapabilityDescriptor::Lifecycle {
            id: lifecycle_id.clone(),
        },
        CapabilityDescriptor::Tool(ToolDescriptor {
            id: tool_id.clone(),
            description: "Search local context bundle".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
                "additionalProperties": false
            }),
            prompt_guidance: "Use for semantic documentation search.".to_string(),
            effects: Vec::new(),
            execution_mode: rho::plugin::contract::ExecutionMode::Sequential,
        }),
    ];

    let manifest = CapabilityManifest {
        plugin_id: "rho-plugin-kiln".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![
            CapabilityDeclaration {
                id: context_id,
                replaces: None,
            },
            CapabilityDeclaration {
                id: command_id,
                replaces: None,
            },
            CapabilityDeclaration {
                id: lifecycle_id,
                replaces: None,
            },
            CapabilityDeclaration {
                id: tool_id,
                replaces: None,
            },
        ],
    };

    let handshake = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Handshake {
            selected_version: PLUGIN_PROTOCOL_VERSION,
        },
    }));
    let discovery = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Discovery { manifest, capabilities },
    }));
    let context_result = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Context(ContextResponse {
            snippets: vec![ContextSnippet {
                source: "sqlite-vec://docs/architecture.md".to_string(),
                title: Some("Kiln Architecture".to_string()),
                content: "Kiln transforms local data into a portable Context Bundle.".to_string(),
                score: Some(0.98),
            }],
        }),
    }));
    let command_result = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Command(CommandInvocationResponse {
            output: "Kiln index synchronized (42 files indexed)".to_string(),
            exit_code: 0,
        }),
    }));
    let lifecycle_result = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Lifecycle,
    }));
    let tool_progress = response(&fragment(ProtocolMessage::StreamEvent {
        event: StreamEvent::Progress {
            message: "Embedding query with local model...".to_string(),
        },
    }));
    let tool_result = response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Tool(ToolInvocationResponse {
            content: "Found 1 matching section in architecture.md".to_string(),
            is_error: false,
            structured_content: None,
        }),
    }));

    let root = std::env::temp_dir().join(format!("rho_kiln_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executable = root.join("rho-plugin-kiln");
    let script = format!(
        r#"#!/bin/sh
read handshake
request_id=$(printf '%s' "$handshake" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
{handshake}read request
request_id=$(printf '%s' "$request" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
case "$request" in
  *\"type\":\"discovery_request\"*) {discovery} ;;
  *\"kind\":\"context\"*) {context_result} ;;
  *\"kind\":\"command\"*) {command_result} ;;
  *\"kind\":\"lifecycle\"*) {lifecycle_result} ;;
  *\"kind\":\"tool\"*) {tool_progress}{tool_result} ;;
esac
"#
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    KilnPluginFixture { root, executable }
}

#[tokio::test]
async fn kiln_plugin_full_capabilities_integration_test() {
    let fixture = fixture();
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();

    let context_id: CapabilityId = "context:kiln".parse().unwrap();
    let context_cap = plugin.context(&context_id).unwrap();
    let context_res = context_cap
        .retrieve(ContextRequest {
            prompt: "how does kiln work".to_string(),
            context: InvocationContext {
                session_id: "sess-kiln".to_string(),
                working_directory: "/workspace".to_string(),
                has_interactive_ui: true,
            },
            token_budget: Some(2048),
        })
        .await
        .unwrap();

    assert_eq!(context_res.snippets.len(), 1);
    assert_eq!(context_res.snippets[0].source, "sqlite-vec://docs/architecture.md");
    assert_eq!(context_res.snippets[0].title.as_deref(), Some("Kiln Architecture"));
    assert!(context_res.snippets[0].content.contains("Context Bundle"));

    let command_id: CapabilityId = "command:kiln".parse().unwrap();
    let command_cap = plugin.command(&command_id).unwrap();
    let cmd_res = command_cap
        .invoke(CommandInvocationRequest {
            arguments: vec!["sync".to_string(), "./docs".to_string()],
            context: InvocationContext {
                session_id: "sess-kiln".to_string(),
                working_directory: "/workspace".to_string(),
                has_interactive_ui: true,
            },
        })
        .await
        .unwrap();

    assert_eq!(cmd_res.exit_code, 0);
    assert!(cmd_res.output.contains("42 files indexed"));

    let lifecycle_id: CapabilityId = "lifecycle:kiln".parse().unwrap();
    let lifecycle_cap = plugin.lifecycle(&lifecycle_id).unwrap();
    lifecycle_cap
        .notify(LifecycleEvent::BeforeTurn {
            session_id: "sess-kiln".to_string(),
            prompt: "what is in docs".to_string(),
            working_directory: "/workspace".to_string(),
        })
        .await
        .unwrap();

    lifecycle_cap
        .notify(LifecycleEvent::AfterTurn {
            session_id: "sess-kiln".to_string(),
            success: true,
        })
        .await
        .unwrap();

    let tool_id: CapabilityId = "tool:kiln_search".parse().unwrap();
    let tool_cap = plugin.tool(&tool_id).unwrap();
    let capturing_host = CapturingHost(Mutex::new(Vec::new()));
    let tool_res = tool_cap
        .invoke(
            &capturing_host,
            ToolInvocationRequest {
                arguments: serde_json::json!({"query": "architecture"}),
                context: InvocationContext {
                    session_id: "sess-kiln".to_string(),
                    working_directory: "/workspace".to_string(),
                    has_interactive_ui: true,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(tool_res.content, "Found 1 matching section in architecture.md");
    assert_eq!(
        capturing_host.0.lock().unwrap().as_slice(),
        &["Embedding query with local model..."]
    );
}
