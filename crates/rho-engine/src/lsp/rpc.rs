//! JSON-RPC 2.0 over the LSP base protocol (`Content-Length` framed messages).

use crate::locking::lock;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

pub type BoxedReader = Box<dyn AsyncRead + Send + Unpin>;
pub type BoxedWriter = Box<dyn AsyncWrite + Send + Unpin>;

/// JSON-RPC "method not found".
pub const METHOD_NOT_FOUND: i64 = -32601;
/// LSP "content modified": the document changed while the server computed the answer.
pub const CONTENT_MODIFIED: i64 = -32801;

/// Receives what the server sends on its own initiative.
pub trait ServerMessages: Send + Sync {
    fn notification(&self, method: &str, params: Value);
    /// The result for a server-to-client request, or `None` when the method is not supported.
    fn request(&self, method: &str, params: &Value) -> Option<Value>;
    /// The connection closed: the server exited or its output ended.
    fn closed(&self);
}

/// Documented cap on one message body; a larger `Content-Length` closes the connection.
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_HEADER_LINE: u64 = 8 * 1024;

#[derive(Debug)]
struct RpcError {
    code: Option<i64>,
    message: String,
}

impl RpcError {
    fn text(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }
}

type Pending = HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>;

pub struct Connection {
    outgoing: mpsc::UnboundedSender<Value>,
    next_id: AtomicI64,
    pending: Mutex<Pending>,
    closed: AtomicBool,
    on_close: tokio::sync::Notify,
}

pub async fn write_message<W: AsyncWrite + Unpin + ?Sized>(writer: &mut W, message: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

fn invalid_data(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_string())
}

async fn read_header_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader.take(MAX_HEADER_LINE).read_line(&mut line).await?;
    if read == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') && read as u64 >= MAX_HEADER_LINE {
        return Err(invalid_data("header line too long"));
    }
    Ok(Some(line))
}

/// The next frame: `None` at end of stream, and an inner error for a body that is not JSON (its
/// bytes are consumed, so the stream stays in sync). Other errors leave the stream unusable.
async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Result<Value, serde_json::Error>>> {
    let mut length = None;
    loop {
        let Some(header) = read_header_line(reader).await? else {
            return Ok(None);
        };
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok().or(length);
        }
    }
    let length = length.ok_or_else(|| invalid_data("message without Content-Length"))?;
    if length > MAX_MESSAGE_BYTES {
        return Err(invalid_data("message too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)))
}

/// The next framed message, or `None` at end of stream.
pub async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<Value>> {
    match read_frame(reader).await? {
        Some(Ok(message)) => Ok(Some(message)),
        Some(Err(error)) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        None => Ok(None),
    }
}

/// Marks the connection closed however its reader or writer task ends, including when the runtime
/// that runs them shuts down.
struct CloseOnDrop {
    connection: Arc<Connection>,
    handler: Arc<dyn ServerMessages>,
}

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.connection.close();
        self.handler.closed();
    }
}

impl Connection {
    pub fn start(reader: BoxedReader, writer: BoxedWriter, handler: Arc<dyn ServerMessages>) -> Arc<Self> {
        let (outgoing, queue) = mpsc::unbounded_channel();
        let connection = Arc::new(Self {
            outgoing,
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
            on_close: tokio::sync::Notify::new(),
        });
        let writer_guard = CloseOnDrop {
            connection: Arc::clone(&connection),
            handler: Arc::clone(&handler),
        };
        tokio::spawn(write_loop(writer, queue, writer_guard));
        let reader_guard = CloseOnDrop {
            connection: Arc::clone(&connection),
            handler,
        };
        tokio::spawn(read_loop(BufReader::new(reader), reader_guard));
        connection
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.pending_map().clear();
        self.on_close.notify_one();
    }

    /// Sends a request and waits for its answer; one that fails with `ContentModified` is asked again once.
    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        match self.request_once(method, params.clone(), timeout).await {
            Err(error) if error.code == Some(CONTENT_MODIFIED) => {
                self.request_once(method, params, timeout).await.map_err(|e| e.message)
            }
            other => other.map_err(|error| error.message),
        }
    }

    async fn request_once(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_map();
            if self.is_closed() {
                return Err(RpcError::text("language server exited"));
            }
            pending.insert(id, tx);
        }
        let mut guard = PendingRequest {
            connection: self,
            id,
            answered: false,
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .map_err(RpcError::text)?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => {
                guard.answered = true;
                result
            }
            Ok(Err(_)) => {
                guard.answered = true;
                Err(RpcError::text("language server exited"))
            }
            Err(_) => Err(RpcError::text(format!(
                "{method} timed out after {}s",
                timeout.as_secs_f32()
            ))),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Queues `message` for the writer task, so no caller ever waits on the server reading its input.
    fn send(&self, message: Value) -> Result<(), String> {
        if self.is_closed() {
            return Err("language server exited".into());
        }
        self.outgoing
            .send(message)
            .map_err(|_| "language server exited".to_string())
    }

    fn pending_map(&self) -> MutexGuard<'_, Pending> {
        lock(&self.pending)
    }

    fn dispatch(&self, message: Value, handler: &dyn ServerMessages) {
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => {
                let reply = match handler.request(&method, message.get("params").unwrap_or(&Value::Null)) {
                    Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    None => json!({ "jsonrpc": "2.0", "id": id,
                        "error": { "code": METHOD_NOT_FOUND, "message": format!("{method} is not supported") } }),
                };
                let _ = self.send(reply);
            }
            (Some(method), None) => {
                handler.notification(&method, message.get("params").cloned().unwrap_or(Value::Null));
            }
            (None, Some(id)) => self.resolve(id.as_i64(), &message),
            (None, None) => {}
        }
    }

    fn resolve(&self, id: Option<i64>, message: &Value) {
        let Some(tx) = id.and_then(|id| self.pending_map().remove(&id)) else {
            return;
        };
        let outcome = match message.get("error") {
            Some(error) => Err(RpcError {
                code: error.get("code").and_then(Value::as_i64),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_string(),
            }),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = tx.send(outcome);
    }
}

async fn read_loop<R: AsyncRead + Unpin>(mut reader: BufReader<R>, guard: CloseOnDrop) {
    while let Ok(Some(frame)) = read_frame(&mut reader).await {
        if let Ok(message) = frame {
            guard.connection.dispatch(message, guard.handler.as_ref());
        }
    }
}

/// Forgets a request that ends without an answer and tells the server to stop working on it.
struct PendingRequest<'a> {
    connection: &'a Connection,
    id: i64,
    answered: bool,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        self.connection.pending_map().remove(&self.id);
        let _ = self.connection.notify("$/cancelRequest", json!({ "id": self.id }));
    }
}

async fn write_loop(mut writer: BoxedWriter, mut queue: mpsc::UnboundedReceiver<Value>, guard: CloseOnDrop) {
    loop {
        let message = tokio::select! {
            message = queue.recv() => message,
            () = guard.connection.on_close.notified() => None,
        };
        let Some(message) = message else {
            return;
        };
        if write_message(&mut writer, &message).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Recorder {
        closed: AtomicUsize,
    }

    impl ServerMessages for Recorder {
        fn notification(&self, _: &str, _: Value) {}
        fn request(&self, method: &str, _: &Value) -> Option<Value> {
            (method == "known").then(|| json!("ok"))
        }
        fn closed(&self) {
            self.closed.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn server_requests_are_answered_or_rejected_while_the_client_is_writing() {
        let (client_side, server_side) = tokio::io::duplex(64);
        let (client_read, client_write) = tokio::io::split(client_side);
        let (server_read, mut server_write) = tokio::io::split(server_side);
        let recorder = Arc::new(Recorder::default());
        let connection = Connection::start(Box::new(client_read), Box::new(client_write), recorder.clone());

        connection.notify("big", json!("x".repeat(10_000))).unwrap();
        write_message(
            &mut server_write,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "known" }),
        )
        .await
        .unwrap();
        write_message(
            &mut server_write,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "other" }),
        )
        .await
        .unwrap();

        let mut reader = BufReader::new(server_read);
        let big = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(big["method"], "big");
        let known = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(known, json!({ "jsonrpc": "2.0", "id": 1, "result": "ok" }));
        let other = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(other["error"]["code"], METHOD_NOT_FOUND);

        drop(server_write);
        drop(reader);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !connection.is_closed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(recorder.closed.load(Ordering::SeqCst) >= 1);
        let after = connection.request("late", Value::Null, Duration::from_secs(5)).await;
        assert_eq!(after, Err("language server exited".to_string()));
    }

    fn pair() -> (
        Arc<Connection>,
        Arc<Recorder>,
        BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
    ) {
        let (client_side, server_side) = tokio::io::duplex(1 << 16);
        let (client_read, client_write) = tokio::io::split(client_side);
        let (server_read, server_write) = tokio::io::split(server_side);
        let recorder = Arc::new(Recorder::default());
        let connection = Connection::start(Box::new(client_read), Box::new(client_write), recorder.clone());
        (connection, recorder, BufReader::new(server_read), server_write)
    }

    #[tokio::test]
    async fn oversized_content_length_is_rejected_before_allocating() {
        let mut huge = BufReader::new(&b"Content-Length: 2147483648\r\n\r\n"[..]);
        let error = read_message(&mut huge).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
    }

    #[tokio::test]
    async fn an_endless_header_line_is_rejected() {
        let mut text = b"X-Pad: ".to_vec();
        text.extend(std::iter::repeat_n(b'a', 20_000));
        text.extend_from_slice(b"\r\nContent-Length: 2\r\n\r\n{}");
        let error = read_message(&mut BufReader::new(&text[..])).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
    }

    #[tokio::test]
    async fn a_bad_duplicate_content_length_keeps_the_valid_one() {
        let mut input = BufReader::new(&b"Content-Length: 2\r\nContent-Length: x\r\n\r\n{}"[..]);
        assert_eq!(read_message(&mut input).await.unwrap(), Some(json!({})));
    }

    #[tokio::test]
    async fn a_malformed_body_is_skipped_and_the_connection_keeps_working() {
        let (connection, _recorder, mut reader, mut server_write) = pair();
        server_write.write_all(b"Content-Length: 4\r\n\r\nnope").await.unwrap();
        write_message(
            &mut server_write,
            &json!({ "jsonrpc": "2.0", "id": 7, "method": "known" }),
        )
        .await
        .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(2), read_message(&mut reader))
            .await
            .expect("the reader survived the bad body")
            .unwrap()
            .unwrap();
        assert_eq!(reply["result"], "ok");
        assert!(!connection.is_closed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_request_is_forgotten_and_canceled() {
        let (connection, _recorder, mut reader, _server_write) = pair();
        let abandoned = tokio::time::timeout(
            Duration::from_millis(10),
            connection.request("slow", Value::Null, Duration::from_secs(30)),
        )
        .await;
        assert!(abandoned.is_err());
        assert!(connection.pending_map().is_empty());
        let first = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(first["method"], "slow");
        let second = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(second["method"], "$/cancelRequest");
        assert_eq!(second["params"]["id"], first["id"]);
    }

    #[tokio::test]
    async fn a_content_modified_error_is_retried_once() {
        let (connection, _recorder, mut reader, mut server_write) = pair();
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let request = read_message(&mut reader).await.unwrap().unwrap();
                let reply = match attempt {
                    0 => json!({ "jsonrpc": "2.0", "id": request["id"],
                        "error": { "code": CONTENT_MODIFIED, "message": "changed" } }),
                    _ => json!({ "jsonrpc": "2.0", "id": request["id"], "result": "fresh" }),
                };
                write_message(&mut server_write, &reply).await.unwrap();
            }
        });
        let answer = connection.request("q", Value::Null, Duration::from_secs(5)).await;
        assert_eq!(answer, Ok(json!("fresh")));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn framing_reads_headers_in_any_case_and_rejects_missing_lengths() {
        let mut ok = BufReader::new(&b"content-length: 2\r\nContent-Type: x\r\n\r\n{}"[..]);
        assert_eq!(read_message(&mut ok).await.unwrap(), Some(json!({})));
        assert_eq!(read_message(&mut ok).await.unwrap(), None);
        let mut bad = BufReader::new(&b"X: 1\r\n\r\n{}"[..]);
        assert!(read_message(&mut bad).await.is_err());
    }
}
