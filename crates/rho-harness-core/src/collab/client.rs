use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use iroh::Endpoint;
use iroh::endpoint::presets::{Minimal, N0};

use crate::collab::CollabSnapshot;
use crate::collab::protocol::COLLAB_ALPN;
use crate::collab::session::{CollabReader, CollabSessionStream, CollabWriter};
use crate::collab::ticket::CollabTicket;
use crate::error::{AppError, Result};

/// Stream alias for the concrete outgoing collaborative QUIC stream.
pub type CollabWriteStream = CollabWriter<iroh::endpoint::SendStream>;

/// Stream alias for the concrete incoming collaborative QUIC stream.
pub type CollabReadStream = CollabReader<iroh::endpoint::RecvStream>;

/// Opaque guest peer endpoint managing lifecycle and connection to a host session.
#[derive(Clone)]
pub struct CollabGuestEndpoint {
    inner: Arc<Endpoint>,
}

impl fmt::Debug for CollabGuestEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CollabGuestEndpoint({})", self.inner.id())
    }
}

impl CollabGuestEndpoint {
    /// Binds a guest P2P endpoint.
    ///
    /// Uses the `Minimal` preset if an explicit local bind address is provided
    /// (e.g. for loopback tests), or `N0` with full NAT traversal if none is given.
    pub async fn bind(bind_addr: Option<SocketAddr>) -> Result<Self> {
        let ep = if let Some(addr) = bind_addr {
            Endpoint::builder(Minimal)
                .bind_addr(addr)
                .map_err(|e| AppError::Network(format!("Failed to configure guest endpoint address: {e}")))?
                .bind()
                .await
                .map_err(|e| AppError::Network(format!("Failed to bind guest endpoint: {e}")))?
        } else {
            Endpoint::builder(N0)
                .bind()
                .await
                .map_err(|e| AppError::Network(format!("Failed to bind guest endpoint: {e}")))?
        };
        Ok(Self { inner: Arc::new(ep) })
    }

    /// Closes the guest endpoint and all active connections.
    pub async fn close(&self) {
        self.inner.close().await;
    }

    /// Connects to a host session specified by `ticket` and performs the handshake.
    pub async fn connect(
        &self,
        ticket: &CollabTicket,
    ) -> Result<(CollabWriteStream, CollabReadStream, Option<CollabSnapshot>)> {
        self.connect_with_hostname(ticket, None).await
    }

    /// Connects to a host session specified by `ticket` with a custom client hostname.
    pub async fn connect_with_hostname(
        &self,
        ticket: &CollabTicket,
        hostname: Option<String>,
    ) -> Result<(CollabWriteStream, CollabReadStream, Option<CollabSnapshot>)> {
        let addr = ticket.to_endpoint_addr();
        let conn = self
            .inner
            .connect(addr, COLLAB_ALPN)
            .await
            .map_err(|e| AppError::Network(format!("Failed to connect to host: {e}")))?;
        let (send, recv) = conn
            .accept_bi()
            .await
            .map_err(|e| AppError::Network(format!("Failed to accept bi-stream: {e}")))?;
        let mut stream =
            CollabSessionStream::connect_guest_with_ticket_and_hostname(send, recv, ticket, hostname).await?;
        let snapshot = stream.recv_snapshot().await?;
        let (writer, reader) = stream.into_split();
        Ok((writer, reader, snapshot))
    }
}
