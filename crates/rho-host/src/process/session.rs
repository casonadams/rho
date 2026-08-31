use super::errors::ProcessError;
use super::framing;
use rho_sdk::capability::PLUGIN_PROTOCOL_VERSION;
use rho_sdk::protocol::{
    Envelope, ErrorCode, MAX_PROTOCOL_RESULT_BYTES, ProtocolMessage, RequestId, ResponseSequenceValidator, StreamEvent,
    TerminalResult, encode_line,
};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

pub struct InvocationSession<'a> {
    pub process: &'a mut SupervisedProcess,
    pub request_id: RequestId,
    pub events: mpsc::Sender<StreamEvent>,
    pub limits: super::client::ProcessLimits,
}

impl InvocationSession<'_> {
    pub async fn run(&mut self, mut cancel: oneshot::Receiver<()>) -> Result<TerminalResult, ProcessError> {
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

pub struct SupervisedProcess {
    pub(crate) child: tokio::process::Child,
    pub(crate) input: tokio::process::ChildStdin,
    pub(crate) output: BufReader<tokio::process::ChildStdout>,
    pub(crate) stderr_seen: Arc<AtomicBool>,
    pub(crate) stderr_task: tokio::task::JoinHandle<()>,
}

impl SupervisedProcess {
    pub async fn spawn(executable: &Path) -> Result<Self, ProcessError> {
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

    pub async fn handshake(&mut self) -> Result<(), ProcessError> {
        self.handshake_with_config(None).await
    }

    pub async fn handshake_with_config(
        &mut self,
        plugin_config: Option<serde_json::Value>,
    ) -> Result<(), ProcessError> {
        let request_id = new_request_id("handshake")?;
        self.write(&Envelope::new(
            request_id.clone(),
            ProtocolMessage::HandshakeRequest {
                supported_versions: vec![PLUGIN_PROTOCOL_VERSION],
                plugin_config,
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

    pub async fn write(&mut self, envelope: &Envelope) -> Result<(), ProcessError> {
        let encoded = encode_line(envelope).map_err(|_| ProcessError::MalformedProtocol)?;
        self.input.write_all(&encoded).await.map_err(map_io_error)?;
        self.input.flush().await.map_err(map_io_error)
    }

    pub async fn stop(&mut self) -> bool {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        self.stderr_task.abort();
        self.stderr_seen.load(Ordering::Relaxed)
    }

    pub async fn fail(&mut self, error: ProcessError) -> ProcessError {
        let stderr = self.stop().await;
        if stderr {
            ProcessError::FailureWithRedactedStderr(error.to_string())
        } else {
            error
        }
    }
}

fn map_io_error(err: std::io::Error) -> ProcessError {
    match err.kind() {
        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::WriteZero => {
            ProcessError::UnexpectedEof
        }
        _ => ProcessError::Io,
    }
}

pub fn new_request_id(prefix: &str) -> Result<RequestId, ProcessError> {
    RequestId::new(format!("{prefix}-{}", uuid::Uuid::new_v4())).map_err(|_| ProcessError::MalformedProtocol)
}
