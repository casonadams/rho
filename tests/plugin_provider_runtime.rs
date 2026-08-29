#![cfg(unix)]

use async_trait::async_trait;
use rho::auth::{AuthStore, Credential};
use rho::config::{Config, PluginConfig};
use rho::engine::provider::host_loop::{
    CancellationSignal, NeutralToolCall, NeutralToolExecutor, NeutralToolResult, NeutralTurnError, NeutralTurnRequest,
    NeutralTurnRuntime, NoopSteeringQueue, NoopTurnObserver, run_neutral_turn,
};
use rho::plugin::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};
use rho::plugin::contract::{
    AuthenticationMethod, CapabilityDescriptor, FinishReason, ModelMetadata, ProviderCapability, ProviderDescriptor,
    ProviderRequest, ProviderStreamEvent,
};
use rho::plugin::external::ExternalPlugin;
use rho::plugin::process::ProcessLimits;
use rho::plugin::protocol::{ProtocolMessage, StreamEvent, TerminalResult};
use rho::ui::TerminalRenderer;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
    request_log: PathBuf,
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

fn stream_event(event: ProviderStreamEvent) -> String {
    response(&fragment(ProtocolMessage::StreamEvent {
        event: StreamEvent::Provider(event),
    }))
}

fn completed() -> String {
    response(&fragment(ProtocolMessage::TerminalResponse {
        result: TerminalResult::StreamCompleted,
    }))
}

/// A configured-only provider plugin whose stream behavior is supplied per test.
fn fixture(invocation: &str) -> Fixture {
    fixture_with(invocation, "")
}

fn fixture_with(invocation: &str, continuation: &str) -> Fixture {
    let capability_id: CapabilityId = "provider:fixture".parse().unwrap();
    let manifest = CapabilityManifest {
        plugin_id: "provider-fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: capability_id.clone(),
            replaces: None,
        }],
    };
    let descriptor = CapabilityDescriptor::Provider(ProviderDescriptor {
        id: capability_id,
        display_name: "Fixture".to_string(),
        models: vec![ModelMetadata {
            id: "fixture-model".to_string(),
            display_name: "Fixture model".to_string(),
            context_limit: Some(4096),
            supports_tools: true,
            supports_images: false,
        }],
        authentication: vec![AuthenticationMethod::None],
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

    let root = std::env::temp_dir().join(format!("rho_provider_runtime_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::write(root.join("input.txt"), "fixture input").unwrap();
    let executable = root.join("plugin");
    let request_log = root.join("config").join("requests.log");
    let script = format!(
        r#"#!/bin/sh
read handshake
request_id=$(printf '%s' "$handshake" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
{handshake}read request
request_id=$(printf '%s' "$request" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
printf '%s\n' "$request" >> '{request_log}'
case "$request" in
  *\"type\":\"discovery_request\"*) {discovery} ;;
  *\"kind\":\"provider_stream\"*\"tool_result\"*) {continuation} ;;
  *\"kind\":\"provider_stream\"*) {invocation} ;;
esac
"#,
        request_log = request_log.display(),
        continuation = continuation,
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fixture {
        root,
        executable,
        request_log,
    }
}

fn engine_config(fixture: &Fixture) -> Config {
    let mut config = Config {
        provider: "fixture".to_string(),
        model: "fixture-model".to_string(),
        config_dir: fixture.root.join("config"),
        sessions_dir: fixture.root.join("config").join("sessions"),
        auth_file: fixture.root.join("config").join("credentials.json"),
        auto_approve: true,
        ..Config::default()
    };
    config.plugins.insert(
        "provider-fixture".to_string(),
        PluginConfig {
            path: fixture.executable.clone(),
            package: None,
            replaces: Default::default(),
            ..Default::default()
        },
    );
    config
}

fn fixture_auth() -> AuthStore {
    let sentinel = "unrelated-credential-sentinel";
    let mut auth = AuthStore::default();
    auth.credentials.insert(
        "openai".to_string(),
        Credential::ApiKey {
            key: sentinel.to_string(),
        },
    );
    auth
}

fn complete_tool_turn() -> String {
    format!(
        "{}{}{}",
        stream_event(ProviderStreamEvent::ToolCall {
            call_id: "fixture-call".to_string(),
            tool_id: "tool:read".parse().unwrap(),
            arguments: serde_json::json!({"path":"input.txt"}),
        }),
        stream_event(ProviderStreamEvent::Finished {
            reason: FinishReason::ToolCalls,
        }),
        completed(),
    )
}

fn continue_turn() -> String {
    format!(
        "{}{}{}{}",
        stream_event(ProviderStreamEvent::TextDelta {
            text: "fixture complete".to_string(),
        }),
        stream_event(ProviderStreamEvent::Usage {
            input_tokens: 7,
            output_tokens: 2,
        }),
        stream_event(ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        }),
        completed(),
    )
}

#[tokio::test]
async fn configured_subprocess_provider_completes_a_host_owned_tool_turn() {
    let fixture = fixture_with(&complete_tool_turn(), &continue_turn());
    let config = engine_config(&fixture);

    let engine = rho::engine::builder::AgentEngineBuilder::new(config, fixture_auth())
        .base_dir(fixture.root.clone())
        .build()
        .await
        .unwrap();
    let output = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("run fixture"),
            &TerminalRenderer::default(),
        )
        .await
        .unwrap();

    assert_eq!(output.final_text, "fixture complete");
    assert_eq!(output.tool_calls_count, 1);
    assert_eq!(output.usage.unwrap().input_tokens, 7);
    let request = std::fs::read_to_string(&fixture.request_log).unwrap();
    assert!(!request.contains("unrelated-credential-sentinel"));
    assert!(!request.contains("AuthStore"));
    assert!(!request.contains("ModelHandle"));
    assert!(!request.contains("credentials.json"));
    let events = engine.session_manager.load_events().await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == rho::session::SessionEventKind::ToolResult)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == rho::session::SessionEventKind::AssistantResponse)
            .count(),
        1
    );
    let canonical = engine.session_manager.load_messages().await.unwrap();
    assert_eq!(canonical.len(), 4);
    assert!(matches!(&canonical[0], rig::message::Message::User { .. }));
    assert!(matches!(&canonical[3], rig::message::Message::Assistant { .. }));
}

#[tokio::test]
async fn second_turn_replays_prior_canonical_history_exactly_once() {
    let fixture = fixture_with(&complete_tool_turn(), &continue_turn());
    let config = engine_config(&fixture);

    let engine = rho::engine::builder::AgentEngineBuilder::new(config, fixture_auth())
        .base_dir(fixture.root.clone())
        .build()
        .await
        .unwrap();
    for prompt in ["first turn", "second turn"] {
        engine
            .run_turn(
                rho::engine::runner::TurnRequest::new(prompt),
                &TerminalRenderer::default(),
            )
            .await
            .unwrap();
    }

    let requests = std::fs::read_to_string(&fixture.request_log).unwrap();
    let second_request = requests.lines().last().unwrap();
    assert_eq!(second_request.matches("first turn").count(), 1);
    assert!(second_request.contains("second turn"));
    let canonical = engine.session_manager.load_messages().await.unwrap();
    assert_eq!(canonical.len(), 6);
    let encoded = serde_json::to_string(&canonical).unwrap();
    assert_eq!(encoded.matches("first turn").count(), 1);
    assert_eq!(encoded.matches("second turn").count(), 1);
    assert_eq!(encoded.matches("fixture complete").count(), 2);
}

#[tokio::test]
async fn crashing_provider_stream_is_isolated_without_partial_commits() {
    let crash = format!(
        "{} exit 3",
        stream_event(ProviderStreamEvent::TextDelta {
            text: "partial".to_string(),
        }),
    );
    let fixture = fixture(&crash);
    let config = engine_config(&fixture);

    let engine = rho::engine::builder::AgentEngineBuilder::new(config, fixture_auth())
        .base_dir(fixture.root.clone())
        .build()
        .await
        .unwrap();
    let error = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("crash"),
            &TerminalRenderer::default(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("provider operation failed"));
    assert!(engine.session_manager.load_messages().await.unwrap().is_empty());
    assert!(
        !engine
            .session_manager
            .load_events()
            .await
            .unwrap()
            .iter()
            .any(|event| event.kind == rho::session::SessionEventKind::AssistantResponse)
    );
}

#[tokio::test]
async fn hanging_provider_stream_is_bounded_by_the_invocation_timeout() {
    let fixture = fixture("cat > /dev/null; sleep 30");
    let plugin = ExternalPlugin::load(
        &fixture.executable,
        ProcessLimits {
            invocation_timeout: Duration::from_millis(400),
            ..ProcessLimits::default()
        },
    )
    .await
    .unwrap();
    let provider = plugin.provider(&"provider:fixture".parse().unwrap()).unwrap();
    let started = std::time::Instant::now();
    let stream = provider
        .stream(ProviderRequest {
            model: "fixture-model".to_string(),
            messages: Vec::new(),
            credential: None,
            max_output_tokens: None,
            tools: Vec::new(),
        })
        .await
        .unwrap();
    use futures::StreamExt;
    let mut stream = stream;
    let outcome = loop {
        match stream.next().await {
            Some(Ok(_)) => continue,
            Some(Err(error)) => break Err(error),
            None => break Ok(()),
        }
    };
    let Err(error) = outcome else {
        panic!("hanging provider must fail");
    };
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn malformed_provider_stream_events_are_rejected() {
    let malformed = stream_event(ProviderStreamEvent::ToolCall {
        call_id: "bad-call".to_string(),
        tool_id: "provider:not-a-tool".parse().unwrap(),
        arguments: serde_json::json!({}),
    });
    let fixture = fixture(&format!("{} {}", malformed, completed()));
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();
    let provider = plugin.provider(&"provider:fixture".parse().unwrap()).unwrap();
    let mut stream = provider
        .stream(ProviderRequest {
            model: "fixture-model".to_string(),
            messages: Vec::new(),
            credential: None,
            max_output_tokens: None,
            tools: Vec::new(),
        })
        .await
        .unwrap();
    use futures::StreamExt;
    let error = loop {
        match stream.next().await {
            Some(Ok(_)) => continue,
            Some(Err(error)) => break error,
            None => panic!("expected a validation failure"),
        }
    };
    assert!(error.to_string().contains("invalid capability response"));
}

#[tokio::test]
async fn cancelled_provider_streams_terminate_the_subprocess() {
    let fixture = fixture("cat > /dev/null; sleep 30");
    let plugin = ExternalPlugin::load(&fixture.executable, ProcessLimits::default())
        .await
        .unwrap();
    let provider = plugin.provider(&"provider:fixture".parse().unwrap()).unwrap();
    let cancellation = CancellationSignal::default();
    cancellation.cancel();
    struct Rejecting;
    #[async_trait]
    impl NeutralToolExecutor for Rejecting {
        async fn execute(&self, _call: NeutralToolCall) -> Result<NeutralToolResult, NeutralTurnError> {
            unreachable!("cancelled turn must not execute tools")
        }
    }
    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &Rejecting,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &NoopSteeringQueue,
        },
        NeutralTurnRequest {
            model: "fixture-model".to_string(),
            messages: Vec::new(),
            credential: None,
            max_output_tokens: None,
            tools: Vec::new(),
            max_turns: 3,
            checkpoint: None,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        terminal,
        rho::engine::provider::host_loop::NeutralTurnTerminal::Cancelled(_)
    ));
}
