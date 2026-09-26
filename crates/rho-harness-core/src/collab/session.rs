use crate::collab::crypto::{CapabilityLevel, CollabSecret};
use crate::collab::protocol::{CollabSnapshot, perform_guest_handshake, perform_host_handshake};
use crate::collab::ticket::CollabTicket;
use crate::error::{AppError, Result};
use crate::rpc::protocol::{RpcCommand, RpcEvent};
use crate::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};

pub struct CollabWriter<W = iroh::endpoint::SendStream> {
    writer: JsonLinesWriter<W>,
    role: CapabilityLevel,
}

impl<W: AsyncWrite + Unpin> CollabWriter<W> {
    #[must_use]
    pub fn new(writer: W, role: CapabilityLevel) -> Self {
        Self {
            writer: JsonLinesWriter::new(writer),
            role,
        }
    }

    #[must_use]
    pub fn role(&self) -> CapabilityLevel {
        self.role
    }

    pub async fn send_event(&mut self, event: &RpcEvent) -> Result<()> {
        self.writer.write_message(event).await
    }

    pub async fn send_command(&mut self, cmd: &RpcCommand) -> Result<()> {
        if self.role == CapabilityLevel::ViewOnly {
            return Err(AppError::Auth("View-only session cannot send commands".into()));
        }
        self.writer.write_message(cmd).await
    }

    pub async fn send_raw<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        self.writer.write_message(msg).await
    }

    pub async fn flush(&mut self) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.writer.get_mut().flush().await?;
        Ok(())
    }

    pub fn get_mut(&mut self) -> &mut W {
        self.writer.get_mut()
    }

    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
}

pub struct CollabReader<R = iroh::endpoint::RecvStream> {
    reader: JsonLinesReader<tokio::io::BufReader<R>>,
    role: CapabilityLevel,
}

impl<R: AsyncRead + Unpin> CollabReader<R> {
    #[must_use]
    pub fn new(reader: R, role: CapabilityLevel) -> Self {
        Self {
            reader: JsonLinesReader::new(tokio::io::BufReader::new(reader)),
            role,
        }
    }

    #[must_use]
    pub fn role(&self) -> CapabilityLevel {
        self.role
    }

    pub async fn recv_event(&mut self) -> Result<Option<RpcEvent>> {
        self.reader.read_message().await
    }

    pub async fn recv_command(&mut self) -> Result<Option<RpcCommand>> {
        if let Some(cmd) = self.recv_raw::<RpcCommand>().await? {
            if self.role == CapabilityLevel::ViewOnly {
                return Err(AppError::Auth("View-only peer attempted to send command".into()));
            }
            return Ok(Some(cmd));
        }
        Ok(None)
    }

    pub async fn recv_raw<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        self.reader.read_message().await
    }

    pub fn get_mut(&mut self) -> &mut tokio::io::BufReader<R> {
        self.reader.get_mut()
    }

    pub fn into_inner(self) -> R {
        self.reader.into_inner().into_inner()
    }
}

pub struct CollabSessionStream<W = iroh::endpoint::SendStream, R = iroh::endpoint::RecvStream> {
    pub writer: CollabWriter<W>,
    pub reader: CollabReader<R>,
}

impl<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> CollabSessionStream<W, R> {
    #[must_use]
    pub fn new(send: W, recv: R, role: CapabilityLevel) -> Self {
        Self {
            writer: CollabWriter::new(send, role),
            reader: CollabReader::new(recv, role),
        }
    }

    #[must_use]
    pub fn role(&self) -> CapabilityLevel {
        self.writer.role()
    }

    #[must_use]
    pub fn into_split(self) -> (CollabWriter<W>, CollabReader<R>) {
        (self.writer, self.reader)
    }

    #[must_use]
    pub fn from_split(writer: CollabWriter<W>, reader: CollabReader<R>) -> Self {
        Self { writer, reader }
    }

    pub async fn send_event(&mut self, event: &RpcEvent) -> Result<()> {
        self.writer.send_event(event).await
    }

    pub async fn recv_event(&mut self) -> Result<Option<RpcEvent>> {
        self.reader.recv_event().await
    }

    pub async fn send_command(&mut self, cmd: &RpcCommand) -> Result<()> {
        self.writer.send_command(cmd).await
    }

    pub async fn recv_command(&mut self) -> Result<Option<RpcCommand>> {
        if self.role() == CapabilityLevel::ViewOnly {
            if let Some(_cmd) = self.reader.recv_raw::<RpcCommand>().await? {
                let err_event = RpcEvent::Error {
                    code: "UNAUTHORIZED".into(),
                    message: "View-only collaborators cannot execute commands".into(),
                };
                self.writer.send_event(&err_event).await?;
                return Err(AppError::Auth("View-only peer attempted to send command".into()));
            }
            return Ok(None);
        }
        self.reader.recv_command().await
    }

    pub async fn send_snapshot(&mut self, snapshot: &CollabSnapshot) -> Result<()> {
        self.send_event(&snapshot.to_rpc_event()).await
    }

    pub async fn recv_snapshot(&mut self) -> Result<Option<CollabSnapshot>> {
        let Some(event) = self.recv_event().await? else {
            return Ok(None);
        };
        CollabSnapshot::try_from(event).map(Some)
    }

    pub async fn accept_host(send: W, recv: R, secret: &CollabSecret) -> Result<Self> {
        let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(recv));
        let mut writer = JsonLinesWriter::new(send);
        let role = perform_host_handshake(&mut reader, &mut writer, secret).await?;
        Ok(Self {
            writer: CollabWriter { writer, role },
            reader: CollabReader { reader, role },
        })
    }

    pub async fn connect_guest(
        send: W,
        recv: R,
        key_material: &[u8; 32],
        is_seed: bool,
        desired_role: CapabilityLevel,
    ) -> Result<Self> {
        let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(recv));
        let mut writer = JsonLinesWriter::new(send);
        let role = perform_guest_handshake(&mut reader, &mut writer, key_material, is_seed, desired_role).await?;
        Ok(Self {
            writer: CollabWriter { writer, role },
            reader: CollabReader { reader, role },
        })
    }

    pub async fn connect_guest_with_ticket(send: W, recv: R, ticket: &CollabTicket) -> Result<Self> {
        let desired_role = if ticket.is_view_only {
            CapabilityLevel::ViewOnly
        } else {
            CapabilityLevel::Full
        };
        let is_seed = !ticket.is_view_only;
        Self::connect_guest(send, recv, &ticket.secret, is_seed, desired_role).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::protocol::COLLAB_ALPN;
    use iroh::Endpoint;
    use iroh::endpoint::presets::Minimal;
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_collab_session_duplex_roundtrip() {
        let secret = CollabSecret::generate().expect("secret");
        let (host_stream, guest_stream) = duplex(1024);
        let (host_read, host_write) = tokio::io::split(host_stream);
        let (guest_read, guest_write) = tokio::io::split(guest_stream);

        let secret_clone = secret.clone();
        let host_fut = CollabSessionStream::accept_host(host_write, host_read, &secret_clone);
        let guest_fut =
            CollabSessionStream::connect_guest(guest_write, guest_read, secret.seed(), true, CapabilityLevel::Full);

        let (mut host_s, mut guest_s) = tokio::try_join!(host_fut, guest_fut).expect("handshake");
        assert_eq!(host_s.role(), CapabilityLevel::Full);
        assert_eq!(guest_s.role(), CapabilityLevel::Full);

        let snapshot = CollabSnapshot::new(vec![serde_json::json!({"turn": 1, "prompt": "duplex test"})], "idle");
        host_s.send_snapshot(&snapshot).await.expect("send snap");
        let received_snap = guest_s.recv_snapshot().await.expect("recv snap");
        assert_eq!(received_snap, Some(snapshot));

        guest_s
            .send_command(&RpcCommand::Prompt {
                message: "ping".into(),
                images: None,
                streaming_behavior: None,
            })
            .await
            .expect("send cmd");
        let received_cmd = host_s.recv_command().await.expect("recv cmd");
        assert!(matches!(received_cmd, Some(RpcCommand::Prompt { message, .. }) if message == "ping"));
    }

    #[tokio::test]
    async fn test_iroh_in_memory_full_session_exchange() {
        use std::net::SocketAddr;
        let host_secret = CollabSecret::generate().expect("generated secret");

        let host_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind host addr")
            .alpns(vec![COLLAB_ALPN.to_vec()])
            .bind()
            .await
            .expect("host bind");
        let host_addr = host_ep.addr();
        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let host_secret_clone = host_secret.clone();
        let host_task = tokio::spawn(async move {
            let incoming = host_ep.accept().await.expect("incoming conn");
            let conn = incoming
                .accept()
                .expect("accept incoming")
                .await
                .expect("conn established");
            let (send, recv) = conn.open_bi().await.expect("open_bi");
            let mut stream = CollabSessionStream::accept_host(send, recv, &host_secret_clone)
                .await
                .expect("accept_host");
            assert_eq!(stream.role(), CapabilityLevel::Full);

            let snapshot = CollabSnapshot::new(vec![serde_json::json!({"turn": 1, "prompt": "build collab"})], "idle");
            stream.send_snapshot(&snapshot).await.expect("send_snapshot");

            stream
                .send_event(&RpcEvent::TextChunk {
                    content: "hello guest".into(),
                })
                .await
                .expect("send_event");

            let cmd = stream.recv_command().await.expect("recv_command");
            assert!(matches!(cmd, Some(RpcCommand::Prompt { message, .. }) if message == "hi host"));

            host_ep.close().await;
        });

        let guest_conn = guest_ep.connect(host_addr, COLLAB_ALPN).await.expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");
        let mut guest_stream =
            CollabSessionStream::connect_guest(send, recv, host_secret.seed(), true, CapabilityLevel::Full)
                .await
                .expect("connect_guest");
        assert_eq!(guest_stream.role(), CapabilityLevel::Full);

        let snap = guest_stream.recv_snapshot().await.expect("recv_snapshot");
        assert_eq!(
            snap,
            Some(CollabSnapshot::new(
                vec![serde_json::json!({"turn": 1, "prompt": "build collab"})],
                "idle"
            ))
        );

        let ev = guest_stream.recv_event().await.expect("recv_event");
        assert!(matches!(ev, Some(RpcEvent::TextChunk { content }) if content == "hello guest"));

        guest_stream
            .send_command(&RpcCommand::Prompt {
                message: "hi host".into(),
                images: None,
                streaming_behavior: None,
            })
            .await
            .expect("send_command");

        host_task.await.expect("host task finished");
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_iroh_in_memory_view_only_rejection() {
        use std::net::SocketAddr;
        let host_secret = CollabSecret::generate().expect("generated secret");
        let read_key = host_secret.derive_read_key().expect("derived read key");

        let host_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind host addr")
            .alpns(vec![COLLAB_ALPN.to_vec()])
            .bind()
            .await
            .expect("host bind");
        let host_addr = host_ep.addr();
        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let host_secret_clone = host_secret.clone();
        let host_task = tokio::spawn(async move {
            let incoming = host_ep.accept().await.expect("incoming conn");
            let conn = incoming
                .accept()
                .expect("accept incoming")
                .await
                .expect("conn established");
            let (send, recv) = conn.open_bi().await.expect("open_bi");
            let mut stream = CollabSessionStream::accept_host(send, recv, &host_secret_clone)
                .await
                .expect("accept_host");
            assert_eq!(stream.role(), CapabilityLevel::ViewOnly);

            let res = stream.recv_command().await;
            assert!(res.is_err());

            let _ = done_rx.await;
            host_ep.close().await;
        });

        let guest_conn = guest_ep.connect(host_addr, COLLAB_ALPN).await.expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");
        let mut guest_stream =
            CollabSessionStream::connect_guest(send, recv, &read_key, false, CapabilityLevel::ViewOnly)
                .await
                .expect("connect_guest");
        assert_eq!(guest_stream.role(), CapabilityLevel::ViewOnly);

        let send_res = guest_stream
            .send_command(&RpcCommand::Prompt {
                message: "illegal".into(),
                images: None,
                streaming_behavior: None,
            })
            .await;
        assert!(send_res.is_err());

        guest_stream
            .writer
            .send_raw(&RpcCommand::Prompt {
                message: "forged prompt".into(),
                images: None,
                streaming_behavior: None,
            })
            .await
            .expect("raw send");

        let ev = guest_stream.recv_event().await.expect("recv error event");
        assert!(matches!(ev, Some(RpcEvent::Error { code, .. }) if code == "UNAUTHORIZED"));

        let _ = done_tx.send(());
        host_task.await.expect("host task finished");
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_connect_with_ticket() {
        let host_secret = CollabSecret::generate().expect("generated secret");
        let (host_stream, guest_stream) = duplex(1024);
        let (host_read, host_write) = tokio::io::split(host_stream);
        let (guest_read, guest_write) = tokio::io::split(guest_stream);

        let secret_key = iroh::SecretKey::generate();
        let endpoint_id = secret_key.public();
        let ticket = CollabTicket::new(endpoint_id, None, false, *host_secret.seed());

        let host_secret_clone = host_secret.clone();
        let host_fut = CollabSessionStream::accept_host(host_write, host_read, &host_secret_clone);
        let guest_fut = CollabSessionStream::connect_guest_with_ticket(guest_write, guest_read, &ticket);

        let (host_s, guest_s) = tokio::try_join!(host_fut, guest_fut).expect("handshake");
        assert_eq!(host_s.role(), CapabilityLevel::Full);
        assert_eq!(guest_s.role(), CapabilityLevel::Full);
    }

    #[tokio::test]
    async fn test_collab_session_split_and_recombine() {
        let (host_stream, guest_stream) = duplex(1024);
        let (host_read, host_write) = tokio::io::split(host_stream);
        let (guest_read, guest_write) = tokio::io::split(guest_stream);

        let stream = CollabSessionStream::new(host_write, host_read, CapabilityLevel::Full);
        let (mut writer, reader) = stream.into_split();
        assert_eq!(writer.role(), CapabilityLevel::Full);
        assert_eq!(reader.role(), CapabilityLevel::Full);

        let mut guest_stream = CollabSessionStream::new(guest_write, guest_read, CapabilityLevel::Full);

        let write_fut = async {
            writer
                .send_event(&RpcEvent::StatusChanged {
                    status: "split_ok".into(),
                })
                .await
        };
        let read_fut = async { guest_stream.recv_event().await };

        let (w_res, r_res) = tokio::join!(write_fut, read_fut);
        w_res.expect("write event");
        assert!(matches!(r_res.expect("read event"), Some(RpcEvent::StatusChanged { status }) if status == "split_ok"));

        let recombined = CollabSessionStream::from_split(writer, reader);
        assert_eq!(recombined.role(), CapabilityLevel::Full);
    }
}
