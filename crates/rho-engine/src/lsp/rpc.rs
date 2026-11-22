//! JSON-RPC 2.0 over the LSP base protocol (`Content-Length` framed messages).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

pub type BoxedReader = Box<dyn AsyncRead + Send + Unpin>;
pub type BoxedWriter = Box<dyn AsyncWrite + Send + Unpin>;

/// Receives what the server sends on its own initiative.
pub trait ServerMessages: Send + Sync {
    fn notification(&self, method: &str, params: Value);
    /// The result to answer a server-to-client request with.
    fn request(&self, method: &str, params: &Value) -> Value;
}

type Pending = Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>;

pub struct Connection {
    writer: tokio::sync::Mutex<BoxedWriter>,
    next_id: AtomicI64,
    pending: Pending,
    closed: AtomicBool,
}

pub async fn write_message(writer: &mut BoxedWriter, message: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

/// The next framed message, or `None` at end of stream.
pub async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).await? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| std::io::Error::other("message without Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

impl Connection {
    pub fn start(reader: BoxedReader, writer: BoxedWriter, handler: Arc<dyn ServerMessages>) -> Arc<Self> {
        let connection = Arc::new(Self {
            writer: tokio::sync::Mutex::new(writer),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(read_loop(Arc::clone(&connection), BufReader::new(reader), handler));
        connection
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if self.is_closed() {
            return Err("language server exited".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending_map().insert(id, tx);
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(error) = self.send(&message).await {
            self.pending_map().remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("language server exited".into()),
            Err(_) => {
                self.pending_map().remove(&id);
                Err(format!("{method} timed out after {}s", timeout.as_secs_f32()))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn send(&self, message: &Value) -> Result<(), String> {
        let mut writer = self.writer.lock().await;
        write_message(&mut writer, message)
            .await
            .map_err(|error| format!("writing to language server: {error}"))
    }

    fn pending_map(&self) -> std::sync::MutexGuard<'_, HashMap<i64, oneshot::Sender<Result<Value, String>>>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn dispatch(&self, message: Value, handler: &dyn ServerMessages) {
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => {
                let result = handler.request(&method, message.get("params").unwrap_or(&Value::Null));
                let _ = self
                    .send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
                    .await;
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
            Some(error) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string()),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = tx.send(outcome);
    }
}

async fn read_loop<R: AsyncRead + Unpin>(
    connection: Arc<Connection>,
    mut reader: BufReader<R>,
    handler: Arc<dyn ServerMessages>,
) {
    while let Ok(Some(message)) = read_message(&mut reader).await {
        connection.dispatch(message, handler.as_ref()).await;
    }
    connection.closed.store(true, Ordering::SeqCst);
    connection.pending_map().clear();
}
