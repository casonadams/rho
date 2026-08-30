mod framing;

use rho_sdk::capability::{PLUGIN_PROTOCOL_VERSION, ValidatedManifest};
use rho_sdk::contract::CapabilityDescriptor;
use rho_sdk::protocol::{
    Envelope, ErrorCode, InvocationRequest, MAX_PROTOCOL_RESULT_BYTES, ProtocolMessage, RequestId,
    ResponseSequenceValidator, StreamEvent, TerminalResult, encode_line,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy)]
pub struct ProcessLimits {
    pub startup_timeout: Duration,
    pub discovery_timeout: Duration,
    pub invocation_timeout: Duration,
    pub cancellation_timeout: Duration,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(5),
            discovery_timeout: Duration::from_secs(5),
            invocation_timeout: Duration::from_secs(60),
            cancellation_timeout: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginProcessClient {
    executable: Arc<PathBuf>,
    limits: ProcessLimits,
}

impl PluginProcessClient {
    pub fn new(executable: impl Into<PathBuf>, limits: ProcessLimits) -> Self {
        Self {
            executable: Arc::new(executable.into()),
            limits,
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub async fn discover(&self) -> Result<ProcessDiscovery, ProcessError> {
        let mut process = self.start().await?;
        let request_id = new_request_id("discovery")?;
        let request = Envelope::new(request_id.clone(), ProtocolMessage::DiscoveryRequest);
        if let Err(error) = process.write(&request).await {
            return Err(process.fail(error).await);
        }
        let response = match tokio::time::timeout(
            self.limits.discovery_timeout,
            framing::read_envelope(&mut process.output),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(process.fail(error).await),
            Err(_) => return Err(process.fail(ProcessError::DiscoveryTimeout).await),
        };
        if response.request_id != request_id {
            return Err(process.fail(ProcessError::CorrelationMismatch).await);
        }
        let ProtocolMessage::TerminalResponse {
            result: TerminalResult::Discovery { manifest, capabilities },
        } = response.message
        else {
            return Err(process.fail(ProcessError::UnexpectedResponse).await);
        };
        let manifest = match manifest.validate() {
            Ok(manifest) => manifest,
            Err(_) => return Err(process.fail(ProcessError::InvalidManifest).await),
        };
        let stderr_redacted = process.stop().await;
        Ok(ProcessDiscovery {
            manifest,
            capabilities,
            stderr_redacted,
        })
    }

    pub async fn start_invocation(
        &self,
        capability_id: rho_sdk::capability::CapabilityId,
        invocation: InvocationRequest,
    ) -> Result<RunningInvocation, ProcessError> {
        let mut process = self.start().await?;
        let request_id = new_request_id("invoke")?;
        let request = Envelope::new(
            request_id.clone(),
            ProtocolMessage::InvocationRequest {
                capability_id,
                invocation,
            },
        );
        if let Err(error) = process.write(&request).await {
            return Err(process.fail(error).await);
        }

        let (event_sender, event_receiver) = mpsc::channel(32);
        let (completion_sender, completion_receiver) = oneshot::channel();
        let (cancel_sender, cancel_receiver) = oneshot::channel();
        let limits = self.limits;
        let task = tokio::spawn(async move {
            let mut invocation = InvocationSession {
                process: &mut process,
                request_id,
                events: event_sender,
                limits,
            };
            let result = tokio::time::timeout(limits.invocation_timeout, invocation.run(cancel_receiver))
                .await
                .unwrap_or(Err(ProcessError::InvocationTimeout));
            let result = match result {
                Ok(terminal) => {
                    process.stop().await;
                    Ok(terminal)
                }
                Err(error) => Err(process.fail(error).await),
            };
            let _ = completion_sender.send(result);
        });
        Ok(RunningInvocation {
            events: event_receiver,
            completion: Some(completion_receiver),
            cancel: Some(cancel_sender),
            task,
        })
    }

    pub async fn invoke(
        &self,
        capability_id: rho_sdk::capability::CapabilityId,
        invocation: InvocationRequest,
    ) -> Result<InvocationOutput, ProcessError> {
        let mut running = self.start_invocation(capability_id, invocation).await?;
        let mut events = Vec::new();
        while let Some(event) = running.next_event().await {
            events.push(event);
        }
        let terminal = running.finish().await?;
        Ok(InvocationOutput { events, terminal })
    }

    async fn start(&self) -> Result<SupervisedProcess, ProcessError> {
        let mut process = SupervisedProcess::spawn(&self.executable).await?;
        let handshake = tokio::time::timeout(self.limits.startup_timeout, process.handshake()).await;
        match handshake {
            Ok(Ok(())) => Ok(process),
            Ok(Err(error)) => Err(process.fail(error).await),
            Err(_) => Err(process.fail(ProcessError::StartupTimeout).await),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDiscovery {
    pub manifest: ValidatedManifest,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub stderr_redacted: bool,
}

impl ProcessDiscovery {
    pub fn validate_strict(&self) -> Result<(), ProcessError> {
        let declared: BTreeSet<_> = self
            .manifest
            .capabilities
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect();
        let mut described = BTreeSet::new();
        for descriptor in &self.capabilities {
            descriptor.validate().map_err(|_| ProcessError::InvalidCapability)?;
            if !described.insert(descriptor.id().clone()) {
                return Err(ProcessError::InvalidCapability);
            }
        }
        if declared == described {
            Ok(())
        } else {
            Err(ProcessError::InvalidCapability)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationOutput {
    pub events: Vec<StreamEvent>,
    pub terminal: TerminalResult,
}

pub struct RunningInvocation {
    events: mpsc::Receiver<StreamEvent>,
    completion: Option<oneshot::Receiver<Result<TerminalResult, ProcessError>>>,
    cancel: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningInvocation {
    pub async fn next_event(&mut self) -> Option<StreamEvent> {
        self.events.recv().await
    }

    pub async fn finish(mut self) -> Result<TerminalResult, ProcessError> {
        self.cancel.take();
        let completion = self.completion.take().ok_or(ProcessError::ProcessTaskFailed)?;
        completion.await.map_err(|_| ProcessError::ProcessTaskFailed)?
    }

    pub async fn cancel(mut self) -> Result<TerminalResult, ProcessError> {
        self.events.close();
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        let completion = self.completion.take().ok_or(ProcessError::ProcessTaskFailed)?;
        completion.await.map_err(|_| ProcessError::ProcessTaskFailed)?
    }
}

impl Drop for RunningInvocation {
    fn drop(&mut self) {
        self.events.close();
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if self.completion.is_none() {
            self.task.abort();
        }
    }
}

struct InvocationSession<'a> {
    process: &'a mut SupervisedProcess,
    request_id: RequestId,
    events: mpsc::Sender<StreamEvent>,
    limits: ProcessLimits,
}

impl InvocationSession<'_> {
    async fn run(&mut self, mut cancel: oneshot::Receiver<()>) -> Result<TerminalResult, ProcessError> {
        let mut sequence = ResponseSequenceValidator::new(self.request_id.clone());
        let mut stream_bytes = 0_usize;
        loop {
            tokio::select! {
                _ = &mut cancel => return self.cancel().await,
                response = framing::read_envelope(&mut self.process.output) => {
                    let response = response?;
                    sequence.accept(&response).map_err(|_| ProcessError::UnexpectedResponse)?;
                    match response.message {
                        ProtocolMessage::StreamEvent { event } => {
                            stream_bytes = stream_bytes.saturating_add(
                                serde_json::to_vec(&event).map_err(|_| ProcessError::MalformedProtocol)?.len(),
                            );
                            if stream_bytes > MAX_PROTOCOL_RESULT_BYTES {
                                return Err(ProcessError::OversizedMessage);
                            }
                            if self.events.send(event).await.is_err() {
                                return self.cancel().await;
                            }
                        }
                        ProtocolMessage::TerminalResponse { result } => {
                            sequence.finish().map_err(|_| ProcessError::UnexpectedResponse)?;
                            return Ok(result);
                        }
                        ProtocolMessage::ErrorResponse { error } => {
                            sequence.finish().map_err(|_| ProcessError::UnexpectedResponse)?;
                            return Err(ProcessError::Remote {
                                code: error.code,
                                retryable: error.retryable,
                            });
                        }
                        _ => return Err(ProcessError::UnexpectedResponse),
                    }
                }
            }
        }
    }

    async fn cancel(&mut self) -> Result<TerminalResult, ProcessError> {
        let cancel_id = new_request_id("cancel")?;
        self.process
            .write(&Envelope::new(
                cancel_id.clone(),
                ProtocolMessage::CancelRequest {
                    target_request_id: self.request_id.clone(),
                },
            ))
            .await?;
        tokio::time::timeout(self.limits.cancellation_timeout, async {
            loop {
                let response = framing::read_envelope(&mut self.process.output).await?;
                if response.request_id == cancel_id {
                    match response.message {
                        ProtocolMessage::TerminalResponse {
                            result: TerminalResult::Cancelled,
                        }
                        | ProtocolMessage::ErrorResponse {
                            error:
                                rho_sdk::protocol::StructuredError {
                                    code: ErrorCode::Cancelled,
                                    ..
                                },
                        } => continue,
                        _ => return Err(ProcessError::UnexpectedResponse),
                    }
                }
                if response.request_id != self.request_id {
                    return Err(ProcessError::CorrelationMismatch);
                }
                match response.message {
                    ProtocolMessage::StreamEvent { event } => {
                        let _ = self.events.send(event).await;
                    }
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Cancelled,
                    } => return Ok(TerminalResult::Cancelled),
                    ProtocolMessage::ErrorResponse { error } if error.code == ErrorCode::Cancelled => {
                        return Ok(TerminalResult::Cancelled);
                    }
                    _ => return Err(ProcessError::UnexpectedResponse),
                }
            }
        })
        .await
        .map_err(|_| ProcessError::CancellationTimeout)?
    }
}

struct SupervisedProcess {
    child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
    stderr_seen: Arc<AtomicBool>,
    stderr_task: tokio::task::JoinHandle<()>,
}

impl SupervisedProcess {
    async fn spawn(executable: &Path) -> Result<Self, ProcessError> {
        let mut child = tokio::process::Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ProcessError::Spawn)?;
        let input = child.stdin.take().ok_or(ProcessError::Spawn)?;
        let output = child.stdout.take().ok_or(ProcessError::Spawn)?;
        let mut stderr = child.stderr.take().ok_or(ProcessError::Spawn)?;
        let stderr_seen = Arc::new(AtomicBool::new(false));
        let observed = stderr_seen.clone();
        let stderr_task = tokio::spawn(async move {
            let mut buffer = [0_u8; 4096];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => observed.store(true, Ordering::Relaxed),
                }
            }
        });
        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
            stderr_seen,
            stderr_task,
        })
    }

    async fn handshake(&mut self) -> Result<(), ProcessError> {
        let request_id = new_request_id("handshake")?;
        self.write(&Envelope::new(
            request_id.clone(),
            ProtocolMessage::HandshakeRequest {
                supported_versions: vec![PLUGIN_PROTOCOL_VERSION],
            },
        ))
        .await?;
        let response = framing::read_envelope(&mut self.output).await?;
        if response.request_id != request_id {
            return Err(ProcessError::CorrelationMismatch);
        }
        match response.message {
            ProtocolMessage::TerminalResponse {
                result: TerminalResult::Handshake { selected_version },
            } if selected_version == PLUGIN_PROTOCOL_VERSION => Ok(()),
            _ => Err(ProcessError::UnsupportedVersion),
        }
    }

    async fn write(&mut self, envelope: &Envelope) -> Result<(), ProcessError> {
        let encoded = encode_line(envelope).map_err(|_| ProcessError::MalformedProtocol)?;
        self.input.write_all(&encoded).await.map_err(|_| ProcessError::Io)?;
        self.input.flush().await.map_err(|_| ProcessError::Io)
    }

    async fn stop(&mut self) -> bool {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        self.stderr_task.abort();
        self.stderr_seen.load(Ordering::Relaxed)
    }

    async fn fail(&mut self, error: ProcessError) -> ProcessError {
        let stderr = self.stop().await;
        if stderr {
            ProcessError::FailureWithRedactedStderr(error.to_string())
        } else {
            error
        }
    }
}

fn new_request_id(prefix: &str) -> Result<RequestId, ProcessError> {
    RequestId::new(format!("{prefix}-{}", uuid::Uuid::new_v4())).map_err(|_| ProcessError::MalformedProtocol)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    #[error("failed to start plugin process")]
    Spawn,
    #[error("plugin process I/O failed")]
    Io,
    #[error("plugin process returned malformed protocol data")]
    MalformedProtocol,
    #[error("plugin process returned an oversized message")]
    OversizedMessage,
    #[error("plugin process closed its output unexpectedly")]
    UnexpectedEof,
    #[error("plugin process returned an unsupported protocol version")]
    UnsupportedVersion,
    #[error("plugin response correlation failed")]
    CorrelationMismatch,
    #[error("plugin process returned an unexpected response")]
    UnexpectedResponse,
    #[error("plugin manifest is invalid")]
    InvalidManifest,
    #[error("plugin capability declaration is invalid")]
    InvalidCapability,
    #[error("plugin startup timed out")]
    StartupTimeout,
    #[error("plugin discovery timed out")]
    DiscoveryTimeout,
    #[error("plugin invocation timed out")]
    InvocationTimeout,
    #[error("plugin cancellation timed out")]
    CancellationTimeout,
    #[error("plugin returned {code:?}; retryable: {retryable}")]
    Remote { code: ErrorCode, retryable: bool },
    #[error("plugin process task failed")]
    ProcessTaskFailed,
    #[error("{0}; plugin stderr diagnostic was redacted")]
    FailureWithRedactedStderr(String),
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use rho_sdk::capability::{CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest};
    use rho_sdk::contract::{
        ExecutionMode, InvocationContext, ToolDescriptor, ToolInvocationRequest, ToolInvocationResponse,
    };
    use rho_sdk::protocol::StructuredError;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        root: PathBuf,
        executable: PathBuf,
        pid_file: PathBuf,
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
            pid_file,
        }
    }

    fn valid_handshake() -> String {
        message_fragment(ProtocolMessage::TerminalResponse {
            result: TerminalResult::Handshake {
                selected_version: PLUGIN_PROTOCOL_VERSION,
            },
        })
    }

    fn tool_discovery() -> (CapabilityManifest, CapabilityDescriptor) {
        let id: rho_sdk::capability::CapabilityId = "tool:fixture".parse().unwrap();
        (
            CapabilityManifest {
                plugin_id: "fixture".parse().unwrap(),
                plugin_version: "1.0.0".to_string(),
                api_version: CAPABILITY_API_VERSION,
                protocol_version: PLUGIN_PROTOCOL_VERSION,
                capabilities: vec![CapabilityDeclaration {
                    id: id.clone(),
                    replaces: None,
                }],
            },
            CapabilityDescriptor::Tool(ToolDescriptor {
                id,
                description: "Fixture tool".to_string(),
                argument_schema: serde_json::json!({"type": "object"}),
                prompt_guidance: String::new(),
                effects: Vec::new(),
                execution_mode: ExecutionMode::Sequential,
            }),
        )
    }

    fn fixture_limits() -> ProcessLimits {
        ProcessLimits {
            startup_timeout: Duration::from_secs(5),
            discovery_timeout: Duration::from_secs(5),
            invocation_timeout: Duration::from_secs(5),
            cancellation_timeout: Duration::from_secs(1),
        }
    }

    async fn wait_for_exit(pid_file: &Path) {
        for _ in 0..50 {
            let exited = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|pid| {
                    std::process::Command::new("kill")
                        .args(["-0", pid.trim()])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .ok()
                })
                .is_none_or(|status| !status.success());
            if exited {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("fixture plugin process remained alive");
    }

    #[tokio::test]
    async fn discovers_and_validates_a_fixture_plugin() {
        let (manifest, descriptor) = tool_discovery();
        let fragment = message_fragment(ProtocolMessage::TerminalResponse {
            result: TerminalResult::Discovery {
                manifest,
                capabilities: vec![descriptor],
            },
        });
        let fixture = fixture(&valid_handshake(), &response_script(&fragment, "discovery"));
        let discovery = PluginProcessClient::new(&fixture.executable, fixture_limits())
            .discover()
            .await
            .unwrap();
        discovery.validate_strict().unwrap();
        assert_eq!(discovery.manifest.plugin_id.as_str(), "fixture");
        wait_for_exit(&fixture.pid_file).await;
    }

    #[tokio::test]
    async fn malformed_eof_and_incompatible_plugins_fail_locally() {
        let malformed = fixture(&valid_handshake(), "read request\nprintf 'not-json\\n'\n");
        assert_eq!(
            PluginProcessClient::new(&malformed.executable, fixture_limits())
                .discover()
                .await,
            Err(ProcessError::MalformedProtocol)
        );
        wait_for_exit(&malformed.pid_file).await;

        let eof = fixture(&valid_handshake(), "read request\nexit 9\n");
        assert_eq!(
            PluginProcessClient::new(&eof.executable, fixture_limits())
                .discover()
                .await,
            Err(ProcessError::UnexpectedEof)
        );
        wait_for_exit(&eof.pid_file).await;

        let incompatible = message_fragment(ProtocolMessage::TerminalResponse {
            result: TerminalResult::Handshake { selected_version: 2 },
        });
        let incompatible = fixture(&incompatible, "");
        assert_eq!(
            PluginProcessClient::new(&incompatible.executable, fixture_limits())
                .discover()
                .await,
            Err(ProcessError::UnsupportedVersion)
        );
        wait_for_exit(&incompatible.pid_file).await;
    }

    #[tokio::test]
    async fn startup_discovery_and_invocation_timeouts_kill_the_process() {
        let startup = fixture("", "read never\n");
        let startup_limits = ProcessLimits {
            startup_timeout: Duration::from_millis(250),
            ..fixture_limits()
        };
        assert_eq!(
            PluginProcessClient::new(&startup.executable, startup_limits)
                .discover()
                .await,
            Err(ProcessError::StartupTimeout)
        );
        wait_for_exit(&startup.pid_file).await;

        let discovery = fixture(&valid_handshake(), "read request\nread never\n");
        let discovery_limits = ProcessLimits {
            discovery_timeout: Duration::from_millis(250),
            ..fixture_limits()
        };
        assert_eq!(
            PluginProcessClient::new(&discovery.executable, discovery_limits)
                .discover()
                .await,
            Err(ProcessError::DiscoveryTimeout)
        );
        wait_for_exit(&discovery.pid_file).await;

        let invocation = fixture(&valid_handshake(), "read request\nread never\n");
        let invocation_limits = ProcessLimits {
            invocation_timeout: Duration::from_millis(250),
            ..fixture_limits()
        };
        let output = PluginProcessClient::new(&invocation.executable, invocation_limits)
            .invoke(
                "tool:fixture".parse().unwrap(),
                InvocationRequest::Tool(ToolInvocationRequest {
                    arguments: serde_json::json!({}),
                    context: InvocationContext {
                        session_id: "session".to_string(),
                        working_directory: "/workspace".to_string(),
                        has_interactive_ui: false,
                    },
                }),
            )
            .await;
        assert_eq!(output, Err(ProcessError::InvocationTimeout));
        wait_for_exit(&invocation.pid_file).await;
    }

    #[tokio::test]
    async fn oversized_output_and_stderr_are_bounded_and_redacted() {
        let oversized = fixture(
            &valid_handshake(),
            "read request\nhead -c 1048577 /dev/zero | tr '\\000' x\nprintf '\\n'\n",
        );
        assert_eq!(
            PluginProcessClient::new(&oversized.executable, fixture_limits())
                .discover()
                .await,
            Err(ProcessError::OversizedMessage)
        );
        wait_for_exit(&oversized.pid_file).await;

        let stderr = fixture(
            &valid_handshake(),
            "read request\nprintf 'credential-value' >&2\nprintf 'not-json\\n'\n",
        );
        let error = PluginProcessClient::new(&stderr.executable, fixture_limits())
            .discover()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("stderr diagnostic was redacted"));
        assert!(!error.contains("credential-value"));
        wait_for_exit(&stderr.pid_file).await;
    }

    #[tokio::test]
    async fn invocation_correlates_stream_events_and_terminal_response() {
        let event = message_fragment(ProtocolMessage::StreamEvent {
            event: StreamEvent::Progress {
                message: "working".to_string(),
            },
        });
        let terminal = message_fragment(ProtocolMessage::TerminalResponse {
            result: TerminalResult::Tool(ToolInvocationResponse {
                content: "done".to_string(),
                is_error: false,
                structured_content: None,
            }),
        });
        let body = format!(
            "read invocation\ninvocation_id=$(printf '%s' \"$invocation\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{event}}}\\n' \"$invocation_id\"\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{terminal}}}\\n' \"$invocation_id\"\n"
        );
        let fixture = fixture(&valid_handshake(), &body);
        let output = PluginProcessClient::new(&fixture.executable, fixture_limits())
            .invoke(
                "tool:fixture".parse().unwrap(),
                InvocationRequest::Tool(ToolInvocationRequest {
                    arguments: serde_json::json!({}),
                    context: InvocationContext {
                        session_id: "session".to_string(),
                        working_directory: "/workspace".to_string(),
                        has_interactive_ui: false,
                    },
                }),
            )
            .await
            .unwrap();
        assert_eq!(output.events.len(), 1);
        assert!(matches!(output.terminal, TerminalResult::Tool(_)));
        wait_for_exit(&fixture.pid_file).await;
    }

    #[tokio::test]
    async fn cancellation_is_sent_and_requires_a_terminal_cancellation() {
        let cancel_ack = message_fragment(ProtocolMessage::TerminalResponse {
            result: TerminalResult::Cancelled,
        });
        let body = format!(
            "read invocation\ninvocation_id=$(printf '%s' \"$invocation\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nread cancel\ncancel_id=$(printf '%s' \"$cancel\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{cancel_ack}}}\\n' \"$cancel_id\"\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",{cancel_ack}}}\\n' \"$invocation_id\"\n"
        );
        let fixture = fixture(&valid_handshake(), &body);
        let running = PluginProcessClient::new(&fixture.executable, fixture_limits())
            .start_invocation("tool:fixture".parse().unwrap(), InvocationRequest::Skills)
            .await
            .unwrap();
        assert_eq!(running.cancel().await.unwrap(), TerminalResult::Cancelled);
        wait_for_exit(&fixture.pid_file).await;
    }

    #[tokio::test]
    async fn remote_error_messages_are_not_exposed() {
        let error = message_fragment(ProtocolMessage::ErrorResponse {
            error: StructuredError::public(ErrorCode::Internal, "credential-value", false),
        });
        let fixture = fixture(&valid_handshake(), &response_script(&error, "invocation"));
        let result = PluginProcessClient::new(&fixture.executable, fixture_limits())
            .invoke("skill:fixture".parse().unwrap(), InvocationRequest::Skills)
            .await
            .unwrap_err()
            .to_string();
        assert!(!result.contains("credential-value"));
        wait_for_exit(&fixture.pid_file).await;
    }
}
