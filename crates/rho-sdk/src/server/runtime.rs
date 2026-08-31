use crate::capability::PLUGIN_PROTOCOL_VERSION;
use crate::contract::{InteractionRequest, InteractionResponse, ToolHost};
use crate::protocol::{
    Envelope, ErrorCode, InvocationRequest, ProtocolMessage, RequestId, StreamEvent, StructuredError, TerminalResult,
    decode_line,
};
use crate::server::builder::Plugin;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

struct ServerToolHost {
    request_id: RequestId,
    stream_tx: tokio::sync::mpsc::UnboundedSender<(RequestId, StreamEvent)>,
}

#[async_trait]
impl ToolHost for ServerToolHost {
    async fn interact(
        &self,
        _request: InteractionRequest,
    ) -> Result<InteractionResponse, crate::capability::CapabilityError> {
        Err(crate::capability::CapabilityError::Unavailable {
            message: "Interactive prompts unavailable in standard tool host".to_string(),
        })
    }

    fn stream_chunk(&self, chunk: &str) {
        let _ = self.stream_tx.send((
            self.request_id.clone(),
            StreamEvent::CommandOutput {
                content: chunk.to_string(),
            },
        ));
    }

    fn progress(&self, message: &str) {
        let _ = self.stream_tx.send((
            self.request_id.clone(),
            StreamEvent::Progress {
                message: message.to_string(),
            },
        ));
    }
}

pub async fn run(plugin: Plugin) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    serve(&plugin, &mut reader, &mut stdout).await
}

pub async fn serve<R, W>(
    plugin: &Plugin,
    reader: &mut R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel::<(RequestId, StreamEvent)>();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let envelope = match decode_line(trimmed.as_bytes()) {
            Ok(env) => env,
            Err(_) => continue,
        };

        let req_id = envelope.request_id;
        let response_msg = match envelope.message {
            ProtocolMessage::HandshakeRequest { supported_versions, .. } => {
                if supported_versions.contains(&PLUGIN_PROTOCOL_VERSION) {
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Handshake {
                            selected_version: PLUGIN_PROTOCOL_VERSION,
                        },
                    }
                } else {
                    ProtocolMessage::ErrorResponse {
                        error: StructuredError::public(
                            ErrorCode::UnsupportedVersion,
                            "Unsupported protocol version",
                            false,
                        ),
                    }
                }
            }
            ProtocolMessage::DiscoveryRequest => ProtocolMessage::TerminalResponse {
                result: TerminalResult::Discovery {
                    manifest: plugin.manifest.clone(),
                    capabilities: plugin.descriptors.clone(),
                },
            },
            ProtocolMessage::InvocationRequest {
                capability_id,
                invocation,
            } => {
                handle_invocation(
                    plugin,
                    InvocationPayload {
                        req_id: req_id.clone(),
                        cap_id: capability_id,
                        invocation,
                    },
                    stream_tx.clone(),
                )
                .await
            }
            ProtocolMessage::CancelRequest { target_request_id } => {
                let cancel_env = Envelope::new(
                    target_request_id,
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Cancelled,
                    },
                );
                let mut bytes = serde_json::to_vec(&cancel_env)?;
                bytes.push(b'\n');
                writer.write_all(&bytes).await?;
                writer.flush().await?;

                ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Cancelled,
                }
            }
            _ => ProtocolMessage::ErrorResponse {
                error: StructuredError::public(ErrorCode::InvalidRequest, "Invalid request message", false),
            },
        };

        while let Ok((event_req_id, event)) = stream_rx.try_recv() {
            let env = Envelope::new(event_req_id, ProtocolMessage::StreamEvent { event });
            let mut bytes = serde_json::to_vec(&env)?;
            bytes.push(b'\n');
            writer.write_all(&bytes).await?;
        }

        let resp_env = Envelope::new(req_id, response_msg);
        let mut bytes = serde_json::to_vec(&resp_env)?;
        bytes.push(b'\n');
        writer.write_all(&bytes).await?;
        writer.flush().await?;
    }

    Ok(())
}

struct InvocationPayload {
    req_id: RequestId,
    cap_id: crate::capability::CapabilityId,
    invocation: InvocationRequest,
}

async fn handle_invocation(
    plugin: &Plugin,
    payload: InvocationPayload,
    stream_tx: tokio::sync::mpsc::UnboundedSender<(RequestId, StreamEvent)>,
) -> ProtocolMessage {
    let InvocationPayload {
        req_id,
        cap_id,
        invocation,
    } = payload;
    match invocation {
        InvocationRequest::Command(req) => match plugin.commands.get(&cap_id) {
            Some(cmd) => match cmd.invoke(req).await {
                Ok(resp) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Command(resp),
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::Context(req) => match plugin.contexts.get(&cap_id) {
            Some(ctx) => match ctx.retrieve(req).await {
                Ok(resp) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Context(resp),
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::Tool(req) => match plugin.tools.get(&cap_id) {
            Some(tool) => {
                let host = ServerToolHost {
                    request_id: req_id,
                    stream_tx,
                };
                match tool.invoke(&host, req).await {
                    Ok(resp) => ProtocolMessage::TerminalResponse {
                        result: TerminalResult::Tool(resp),
                    },
                    Err(e) => ProtocolMessage::ErrorResponse {
                        error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                    },
                }
            }
            None => not_found(cap_id),
        },
        InvocationRequest::Lifecycle(event) => match plugin.lifecycles.get(&cap_id) {
            Some(l) => match l.notify(event).await {
                Ok(()) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Lifecycle,
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::Permission(op) => match plugin.permissions.get(&cap_id) {
            Some(perm) => match perm.evaluate(op).await {
                Ok(decision) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Permission(decision),
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::Skills => match plugin.skills.get(&cap_id) {
            Some(skill) => match skill.assets().await {
                Ok(assets) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::Skills(assets),
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::ProviderAuthenticate(req) => match plugin.providers.get(&cap_id) {
            Some(prov) => match prov.authenticate(req).await {
                Ok(resp) => ProtocolMessage::TerminalResponse {
                    result: TerminalResult::ProviderAuthenticated(resp),
                },
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
        InvocationRequest::ProviderStream(req) => match plugin.providers.get(&cap_id) {
            Some(prov) => match prov.stream(req).await {
                Ok(mut stream) => {
                    while let Some(event_res) = stream.next().await {
                        match event_res {
                            Ok(event) => {
                                let _ = stream_tx.send((req_id.clone(), StreamEvent::Provider(event)));
                            }
                            Err(e) => {
                                return ProtocolMessage::ErrorResponse {
                                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                                };
                            }
                        }
                    }
                    ProtocolMessage::TerminalResponse {
                        result: TerminalResult::StreamCompleted,
                    }
                }
                Err(e) => ProtocolMessage::ErrorResponse {
                    error: StructuredError::public(ErrorCode::Internal, e.to_string(), false),
                },
            },
            None => not_found(cap_id),
        },
    }
}

fn not_found(id: crate::capability::CapabilityId) -> ProtocolMessage {
    ProtocolMessage::ErrorResponse {
        error: StructuredError::public(
            ErrorCode::CapabilityNotFound,
            format!("Capability not registered: {id}"),
            false,
        ),
    }
}
