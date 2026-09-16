use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
use rho_harness_core::rpc::ticket::{extract_ticket_b64, parse_ticket_info};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const RHO_ALPN: &[u8] = b"/rho/rpc/v1";
static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);

type ResponseWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>>;

#[derive(Clone)]
pub struct WebClient {
    endpoint_id: String,
    ticket: String,
    tx: mpsc::UnboundedSender<String>,
    waiters: ResponseWaiters,
    transport_name: String,
}

impl PartialEq for WebClient {
    fn eq(&self, other: &Self) -> bool {
        self.endpoint_id == other.endpoint_id && self.ticket == other.ticket
    }
}

impl WebClient {
    pub fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    pub fn ticket(&self) -> &str {
        &self.ticket
    }

    pub fn transport_name(&self) -> &str {
        &self.transport_name
    }

    pub async fn connect(ticket_str: &str, event_tx: mpsc::UnboundedSender<RpcEvent>) -> Result<Self, String> {
        let parsed = parse_ticket_info(ticket_str).map_err(|e| format!("Invalid node ticket: {e}"))?;
        let endpoint_id = parsed.endpoint_id.clone();
        let waiters: ResponseWaiters = Arc::new(Mutex::new(HashMap::new()));

        match Self::try_connect_iroh(ticket_str, &parsed, waiters.clone(), event_tx.clone()).await {
            Ok(tx) => Ok(Self {
                endpoint_id,
                ticket: ticket_str.to_string(),
                tx,
                waiters,
                transport_name: "iroh".to_string(),
            }),
            Err(iroh_err) => {
                web_sys::console::warn_1(&format!("Iroh P2P failed ({iroh_err}), trying WebSocket fallback...").into());
                let tx = Self::connect_websocket(&parsed, waiters.clone(), event_tx).await?;
                Ok(Self {
                    endpoint_id,
                    ticket: ticket_str.to_string(),
                    tx,
                    waiters,
                    transport_name: "websocket".to_string(),
                })
            }
        }
    }

    async fn try_connect_iroh(
        ticket_str: &str,
        _parsed: &rho_harness_core::rpc::ticket::ParsedTicket,
        waiters: ResponseWaiters,
        event_tx: mpsc::UnboundedSender<RpcEvent>,
    ) -> Result<mpsc::UnboundedSender<String>, String> {
        let raw = extract_ticket_b64(ticket_str);
        let bytes = URL_SAFE_NO_PAD
            .decode(raw)
            .map_err(|e| format!("invalid base64 ticket: {e}"))?;
        let addr: iroh::EndpointAddr =
            serde_json::from_slice(&bytes).map_err(|e| format!("invalid endpoint addr json: {e}"))?;

        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![RHO_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| format!("failed to bind iroh browser endpoint: {e}"))?;

        let conn = endpoint
            .connect(addr, RHO_ALPN)
            .await
            .map_err(|e| format!("failed to connect to peer over iroh: {e}"))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| format!("failed to open bi stream over iroh: {e}"))?;

        let _ = send.write_all(b"\n").await;

        let (tx, mut rx) = mpsc::unbounded::<String>();

        wasm_bindgen_futures::spawn_local(async move {
            while let Some(msg) = rx.next().await {
                let mut data = msg.into_bytes();
                if !data.ends_with(b"\n") {
                    data.push(b'\n');
                }
                if send.write_all(&data).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });

        wasm_bindgen_futures::spawn_local(async move {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match recv.read(&mut chunk).await {
                    Ok(Some(n)) if n > 0 => {
                        buf.extend_from_slice(&chunk[..n]);
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = buf.drain(..=pos).collect();
                            if let Ok(s) = String::from_utf8(line) {
                                let trimmed = s.trim();
                                if !trimmed.is_empty() {
                                    dispatch_raw_frame(trimmed, &waiters, &event_tx);
                                }
                            }
                        }
                    }
                    _ => break,
                }
            }
        });

        Ok(tx)
    }

    async fn connect_websocket(
        parsed: &rho_harness_core::rpc::ticket::ParsedTicket,
        waiters: ResponseWaiters,
        event_tx: mpsc::UnboundedSender<RpcEvent>,
    ) -> Result<mpsc::UnboundedSender<String>, String> {
        let ws_port = parsed.ws_port.unwrap_or(50051);
        let host = if let Some(first) = parsed.direct_addresses.first() {
            first.split(':').next().unwrap_or("127.0.0.1")
        } else {
            "127.0.0.1"
        };
        let target_url = format!("ws://{host}:{ws_port}");

        let ws = web_sys::WebSocket::new(&target_url).map_err(|e| format!("Failed to create WebSocket: {e:?}"))?;

        let (open_tx, open_rx) = oneshot::channel::<Result<(), String>>();
        let open_tx = Arc::new(Mutex::new(Some(open_tx)));

        let open_tx_clone = open_tx.clone();
        let onopen = Closure::wrap(Box::new(move || {
            if let Ok(mut lock) = open_tx_clone.lock()
                && let Some(tx) = lock.take()
            {
                let _ = tx.send(Ok(()));
            }
        }) as Box<dyn FnMut()>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let onerror = Closure::wrap(Box::new(move |e: JsValue| {
            if let Ok(mut lock) = open_tx.lock()
                && let Some(tx) = lock.take()
            {
                let _ = tx.send(Err(format!("WebSocket connection error: {e:?}")));
            }
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let waiters_clone = waiters.clone();
        let event_tx_clone = event_tx.clone();
        let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
            if let Ok(txt) = e.data().dyn_into::<js_sys::JsString>() {
                let s: String = txt.into();
                dispatch_raw_frame(s.trim(), &waiters_clone, &event_tx_clone);
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        open_rx
            .await
            .map_err(|_| "WebSocket connection cancelled".to_string())??;

        let (tx, mut rx) = mpsc::unbounded::<String>();
        let ws_clone = ws.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(msg) = rx.next().await {
                let _ = ws_clone.send_with_str(&msg);
            }
            let _ = ws_clone.close();
        });

        Ok(tx)
    }

    pub async fn send_command(&self, cmd: RpcCommand) -> Result<RpcResponse, String> {
        let req_id = format!("req-{}", REQ_COUNTER.fetch_add(1, Ordering::SeqCst));
        let request = RpcRequest {
            id: Some(req_id.clone()),
            command: cmd,
        };

        let json = serde_json::to_string(&request).map_err(|e| format!("Serialization error: {e}"))?;

        let (resp_tx, resp_rx) = oneshot::channel();
        if let Ok(mut map) = self.waiters.lock() {
            map.insert(req_id.clone(), resp_tx);
        }

        self.tx.unbounded_send(json).map_err(|e| format!("Send error: {e}"))?;

        resp_rx.await.map_err(|_| "Response waiter dropped".to_string())
    }
}

fn dispatch_raw_frame(line: &str, waiters: &ResponseWaiters, event_tx: &mpsc::UnboundedSender<RpcEvent>) {
    if line.is_empty() {
        return;
    }

    if let Ok(resp) = serde_json::from_str::<RpcResponse>(line)
        && resp.r#type == "response"
        && let Some(id) = &resp.id
    {
        let waiter = if let Ok(mut map) = waiters.lock() {
            map.remove(id)
        } else {
            None
        };
        if let Some(w) = waiter {
            let _ = w.send(resp);
            return;
        }
    }

    if let Ok(ev) = serde_json::from_str::<RpcEvent>(line) {
        let _ = event_tx.unbounded_send(ev);
    } else {
        web_sys::console::warn_1(&format!("Unknown RPC frame: {line}").into());
    }
}
