use super::client::{InvocationOutput, PluginProcessClient, ProcessLimits};
use super::errors::ProcessError;
use rho_sdk::capability::{CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest};
use rho_sdk::contract::{
    ExecutionMode, InvocationContext, ToolDescriptor, ToolInvocationRequest, ToolInvocationResponse,
};
use rho_sdk::protocol::{ErrorCode, InvocationRequest, ProtocolMessage, StreamEvent, StructuredError, TerminalResult};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
    _pid_file: PathBuf,
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

fn response_script(fragment: &str, input: &str) -> String {
    format!(
        "read {input}\n{input}_id=$(printf '%s' \"${input}\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{fragment}}}\\n' \"${input}_id\"\n"
    )
}

fn fixture(handshake: &str, body: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("rho_process_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let executable = root.join("plugin");
    let pid_file = root.join("pid");
    let handshake_script = if handshake.is_empty() {
        "read handshake\n".to_string()
    } else {
        response_script(handshake, "handshake")
    };
    let script = format!(
        "#!/bin/sh\necho $$ > '{}'\n{}{}",
        pid_file.display(),
        handshake_script,
        body
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fixture {
        root,
        executable,
        _pid_file: pid_file,
    }
}

fn discovery_fixture() -> (String, ToolDescriptor, CapabilityManifest) {
    let tool_id = "tool:fixture".parse().unwrap();
    let descriptor = ToolDescriptor {
        id: tool_id,
        description: "Fixture".to_string(),
        argument_schema: serde_json::json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string"}},
            "additionalProperties": false
        }),
        prompt_guidance: String::new(),
        effects: Vec::new(),
        execution_mode: ExecutionMode::Sequential,
    };
    let manifest = CapabilityManifest {
        plugin_id: "process-fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: rho_sdk::capability::PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: descriptor.id.clone(),
            replaces: None,
        }],
    };
    let discovery = message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Discovery {
            manifest: manifest.clone(),
            capabilities: vec![rho_sdk::contract::CapabilityDescriptor::Tool(descriptor.clone())],
        },
    });
    (discovery, descriptor, manifest)
}

fn handshake_fragment() -> String {
    message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Handshake {
            selected_version: rho_sdk::capability::PLUGIN_PROTOCOL_VERSION,
        },
    })
}

fn tool_invocation_request() -> InvocationRequest {
    InvocationRequest::Tool(ToolInvocationRequest {
        arguments: serde_json::json!({"message": "hello"}),
        context: InvocationContext {
            session_id: "session".to_string(),
            working_directory: "/workspace".to_string(),
            has_interactive_ui: false,
        },
    })
}

#[tokio::test]
async fn discovers_and_validates_a_fixture_plugin() {
    let (discovery, descriptor, manifest) = discovery_fixture();
    let fixture = fixture(&handshake_fragment(), &response_script(&discovery, "discovery"));
    let client = PluginProcessClient::new(
        &fixture.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            ..ProcessLimits::default()
        },
    );
    let output = client.discover().await.unwrap();
    assert_eq!(output.manifest, manifest.validate().unwrap());
    assert_eq!(
        output.capabilities,
        vec![rho_sdk::contract::CapabilityDescriptor::Tool(descriptor)]
    );
}

#[tokio::test]
async fn invocation_correlates_stream_events_and_terminal_response() {
    let stream_event = message_fragment(ProtocolMessage::StreamEvent {
        event: StreamEvent::Progress {
            message: "step".to_string(),
        },
    });
    let terminal_response = message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Tool(ToolInvocationResponse {
            content: "ok".to_string(),
            is_error: false,
            structured_content: None,
        }),
    });
    let body = format!(
        "read request\nrequest_id=$(printf '%s' \"$request\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{stream_event}}}\\n' \"$request_id\"\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{terminal_response}}}\\n' \"$request_id\"\n"
    );
    let fixture = fixture(&handshake_fragment(), &body);
    let client = PluginProcessClient::new(
        &fixture.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            ..ProcessLimits::default()
        },
    );
    let result = client
        .invoke("tool:fixture".parse().unwrap(), tool_invocation_request())
        .await
        .unwrap();
    assert_eq!(
        result,
        InvocationOutput {
            events: vec![StreamEvent::Progress {
                message: "step".to_string()
            }],
            terminal: TerminalResult::Tool(ToolInvocationResponse {
                content: "ok".to_string(),
                is_error: false,
                structured_content: None,
            }),
        }
    );
}

#[tokio::test]
async fn cancellation_is_sent_and_requires_a_terminal_cancellation() {
    let body = r#"read request
request_id=$(printf '%s' "$request" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
read cancel
cancel_id=$(printf '%s' "$cancel" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
printf '{"protocol_version":1,"request_id":"%s","type":"terminal_response","result":{"kind":"cancelled"}}\n' "$request_id"
printf '{"protocol_version":1,"request_id":"%s","type":"terminal_response","result":{"kind":"cancelled"}}\n' "$cancel_id"
"#;
    let fixture = fixture(&handshake_fragment(), body);
    let client = PluginProcessClient::new(
        &fixture.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            ..ProcessLimits::default()
        },
    );
    let running = client
        .start_invocation("tool:fixture".parse().unwrap(), tool_invocation_request())
        .await
        .unwrap();
    let result = running.cancel().await.unwrap();
    assert_eq!(result, TerminalResult::Cancelled);
}

#[tokio::test]
async fn malformed_eof_and_incompatible_plugins_fail_locally() {
    let eof = fixture(&handshake_fragment(), "");
    let client = PluginProcessClient::new(&eof.executable, ProcessLimits::default());
    assert_eq!(client.discover().await.unwrap_err(), ProcessError::UnexpectedEof);

    let malformed_message = message_fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::Handshake { selected_version: 99 },
    });
    let malformed = fixture(&malformed_message, "");
    let client = PluginProcessClient::new(&malformed.executable, ProcessLimits::default());
    assert_eq!(client.discover().await.unwrap_err(), ProcessError::UnsupportedVersion);
}

#[tokio::test]
async fn startup_discovery_and_invocation_timeouts_kill_the_process() {
    let startup_hang = fixture("", "sleep 10\n");
    let client = PluginProcessClient::new(
        &startup_hang.executable,
        ProcessLimits {
            startup_timeout: Duration::from_millis(250),
            ..ProcessLimits::default()
        },
    );
    assert_eq!(client.discover().await.unwrap_err(), ProcessError::StartupTimeout);

    let discovery_hang = fixture(&handshake_fragment(), "read request\nsleep 10\n");
    let client = PluginProcessClient::new(
        &discovery_hang.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            discovery_timeout: Duration::from_millis(250),
            ..ProcessLimits::default()
        },
    );
    assert_eq!(client.discover().await.unwrap_err(), ProcessError::DiscoveryTimeout);

    let invoke_hang = fixture(&handshake_fragment(), "read request\nsleep 10\n");
    let client = PluginProcessClient::new(
        &invoke_hang.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            invocation_timeout: Duration::from_millis(250),
            ..ProcessLimits::default()
        },
    );
    assert_eq!(
        client
            .invoke("tool:fixture".parse().unwrap(), tool_invocation_request())
            .await
            .unwrap_err(),
        ProcessError::InvocationTimeout
    );
}

#[tokio::test]
async fn remote_error_messages_are_not_exposed() {
    let error = message_fragment(ProtocolMessage::ErrorResponse {
        error: StructuredError::redacted(ErrorCode::Internal, false),
    });
    let fixture = fixture(&handshake_fragment(), &response_script(&error, "invoke"));
    let client = PluginProcessClient::new(
        &fixture.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            ..ProcessLimits::default()
        },
    );
    let err = client
        .invoke("tool:fixture".parse().unwrap(), tool_invocation_request())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ProcessError::Remote {
            code: ErrorCode::Internal,
            retryable: false,
        }
    );
}

#[tokio::test]
async fn oversized_output_and_stderr_are_bounded_and_redacted() {
    let stderr_body = "echo 'secret token line' >&2\n";
    let fixture = fixture(&handshake_fragment(), stderr_body);
    let client = PluginProcessClient::new(
        &fixture.executable,
        ProcessLimits {
            startup_timeout: Duration::from_secs(10),
            ..ProcessLimits::default()
        },
    );
    let error = client
        .invoke("tool:fixture".parse().unwrap(), tool_invocation_request())
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret token line"));
    assert!(error.contains("stderr diagnostic was redacted"));
}
