use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Inline input shown at the bottom of the same modal when this option is
    /// chosen; the submitted text returns with the selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<SelectInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectInput {
    /// Label for the input line (e.g. "edit", "pattern", "reason").
    pub label: String,
    /// Prefill for the input buffer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl SelectOption {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
            input: None,
        }
    }

    pub fn with_description(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: Some(description.into()),
            input: None,
        }
    }

    pub fn with_input(
        label: impl Into<String>,
        description: impl Into<String>,
        input_label: impl Into<String>,
        input_value: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            description: Some(description.into()),
            input: Some(SelectInput {
                label: input_label.into(),
                value: input_value,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectResult {
    Selected(usize),
    /// Text submitted through an option's inline input; carries both so the
    /// plugin knows which action the text belongs to.
    SelectedWithInput {
        index: usize,
        text: String,
    },
    Custom(String),
    Cancelled,
}

#[derive(Clone)]
pub struct HostContext {
    pub(crate) out_tx: mpsc::Sender<String>,
    pub(crate) pending_rpc: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    pub(crate) next_id: Arc<AtomicU64>,
}

impl HostContext {
    pub fn noop() -> Self {
        let (out_tx, _) = mpsc::channel(1);
        Self {
            out_tx,
            pending_rpc: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub async fn confirm(&self, title: &str, message: &str) -> bool {
        let params = json!({
            "title": title,
            "message": message,
        });
        let res = self.call_host("host/ui/confirm", params).await;
        res.get("confirmed").and_then(Value::as_bool).unwrap_or(false)
    }

    pub async fn select(
        &self,
        title: &str,
        message: &str,
        options: &[SelectOption],
        allow_custom: bool,
    ) -> SelectResult {
        let params = json!({
            "title": title,
            "message": message,
            "options": options,
            "allow_custom": allow_custom,
        });
        let res = self.call_host("host/ui/select", params).await;
        if let Some(idx) = res.get("selected").and_then(Value::as_u64)
            && let Some(text) = res.get("custom").and_then(Value::as_str)
        {
            SelectResult::SelectedWithInput {
                index: idx as usize,
                text: text.to_string(),
            }
        } else if let Some(idx) = res.get("selected").and_then(Value::as_u64) {
            SelectResult::Selected(idx as usize)
        } else if let Some(custom) = res.get("custom").and_then(Value::as_str) {
            SelectResult::Custom(custom.to_string())
        } else {
            SelectResult::Cancelled
        }
    }

    pub async fn input(&self, title: &str, message: &str) -> Option<String> {
        let params = json!({
            "title": title,
            "message": message,
        });
        let res = self.call_host("host/ui/input", params).await;
        res.get("value").and_then(Value::as_str).map(str::to_string)
    }

    /// Like `input`, but prefills the editable text buffer with `value` so the
    /// user modifies an existing input instead of retyping it. Older hosts
    /// ignore the extra field and behave like `input`.
    pub async fn input_with_default(&self, title: &str, message: &str, value: &str) -> Option<String> {
        let params = json!({
            "title": title,
            "message": message,
            "value": value,
        });
        let res = self.call_host("host/ui/input", params).await;
        res.get("value").and_then(Value::as_str).map(str::to_string)
    }

    pub async fn notify(&self, message: &str, level: &str) {
        let params = json!({
            "message": message,
            "level": level,
        });
        let _ = self.call_host("host/ui/notify", params).await;
    }

    pub async fn block(&self, title: &str, content: &str, style: &str) {
        let params = json!({
            "title": title,
            "content": content,
            "style": style,
        });
        let _ = self.call_host("host/ui/block", params).await;
    }

    pub async fn set_status(&self, key: &str, text: Option<&str>) {
        let params = json!({
            "key": key,
            "text": text,
        });
        let _ = self.call_host("host/ui/set_status", params).await;
    }

    pub async fn get_all_tools(&self) -> Vec<ToolInfo> {
        let res = self.call_host("host/tools/list", json!({})).await;
        res.get("tools")
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .unwrap_or_default()
    }

    async fn call_host(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending_rpc.lock().await;
            map.insert(id, tx);
        }

        if self.out_tx.send(req.to_string()).await.is_err() {
            return Value::Null;
        }

        rx.await.unwrap_or(Value::Null)
    }
}
