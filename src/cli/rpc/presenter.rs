use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, PoisonError};

use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay};
use rho_harness_core::rpc::RpcEventSink;
use rho_harness_core::rpc::protocol::RpcEvent;
use serde_json::Value;
use tokio::sync::{Mutex, Notify, oneshot};

pub type PendingApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<InteractionResponse>>>>;

const MAX_BUFFERED_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Events waiting for the client. Text and reasoning chunks join the tail chunk of the same kind,
/// and once buffered text passes the cap the oldest is dropped behind an `OutputTruncated` event
/// at the point it was dropped from.
#[derive(Clone)]
pub struct RpcEventQueue {
    shared: Arc<QueueShared>,
}

struct QueueShared {
    state: std::sync::Mutex<QueueState>,
    ready: Notify,
}

struct QueueState {
    events: VecDeque<RpcEvent>,
    text_bytes: usize,
    text_byte_cap: usize,
    closed: bool,
}

fn text_content(event: &mut RpcEvent) -> Option<&mut String> {
    match event {
        RpcEvent::TextChunk { content } | RpcEvent::ReasoningChunk { content } => Some(content),
        _ => None,
    }
}

impl QueueState {
    fn push(&mut self, mut event: RpcEvent) {
        self.text_bytes += text_content(&mut event).map_or(0, |content| content.len());
        match (self.events.back_mut(), event) {
            (Some(RpcEvent::TextChunk { content }), RpcEvent::TextChunk { content: more })
            | (Some(RpcEvent::ReasoningChunk { content }), RpcEvent::ReasoningChunk { content: more }) => {
                content.push_str(&more);
            }
            (_, event) => self.events.push_back(event),
        }
        if self.text_bytes > self.text_byte_cap {
            self.drop_oldest_text(self.text_byte_cap / 2);
        }
    }

    /// Trims to `target` rather than to the cap, so a stalled client costs one large move per
    /// half cap of tokens instead of one per token.
    fn drop_oldest_text(&mut self, target: usize) {
        while self.text_bytes > target {
            let excess = self.text_bytes - target;
            let Some((index, content)) = self
                .events
                .iter_mut()
                .enumerate()
                .find_map(|(index, event)| text_content(event).map(|content| (index, content)))
            else {
                return;
            };
            let cut = (excess..content.len())
                .find(|&at| content.is_char_boundary(at))
                .unwrap_or(content.len());
            self.text_bytes -= cut;
            if cut == content.len() {
                self.events.remove(index);
            } else {
                content.drain(..cut);
            }
            self.record_dropped(index, cut);
        }
    }

    fn record_dropped(&mut self, index: usize, bytes: usize) {
        let preceding = index.checked_sub(1).and_then(|before| self.events.get_mut(before));
        let marker = match preceding {
            Some(RpcEvent::OutputTruncated { dropped_bytes }) => Some(dropped_bytes),
            _ => None,
        };
        match marker {
            Some(dropped_bytes) => *dropped_bytes += bytes,
            None => self
                .events
                .insert(index, RpcEvent::OutputTruncated { dropped_bytes: bytes }),
        }
    }

    fn take_all(&mut self) -> Vec<RpcEvent> {
        self.text_bytes = 0;
        self.events.drain(..).collect()
    }
}

impl Default for RpcEventQueue {
    fn default() -> Self {
        Self::with_text_byte_cap(MAX_BUFFERED_TEXT_BYTES)
    }
}

impl RpcEventQueue {
    pub(crate) fn with_text_byte_cap(text_byte_cap: usize) -> Self {
        let state = QueueState {
            events: VecDeque::new(),
            text_bytes: 0,
            text_byte_cap,
            closed: false,
        };
        Self {
            shared: Arc::new(QueueShared {
                state: std::sync::Mutex::new(state),
                ready: Notify::new(),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues `event` unless the queue is closed; returns whether it was queued.
    pub fn push(&self, event: RpcEvent) -> bool {
        {
            let mut state = self.state();
            if state.closed {
                return false;
            }
            state.push(event);
        }
        self.shared.ready.notify_one();
        true
    }

    /// Refuses further events and ends `next_batch` once the queued ones are taken.
    pub fn close(&self) {
        self.state().closed = true;
        self.shared.ready.notify_one();
    }

    /// Every queued event, waiting for one if none is queued; `None` once closed and empty.
    pub async fn next_batch(&self) -> Option<Vec<RpcEvent>> {
        loop {
            {
                let mut state = self.state();
                if !state.events.is_empty() {
                    return Some(state.take_all());
                }
                if state.closed {
                    return None;
                }
            }
            self.shared.ready.notified().await;
        }
    }
}

impl RpcEventSink for RpcEventQueue {
    fn send(&self, event: RpcEvent) -> bool {
        self.push(event)
    }
}

#[derive(Clone)]
pub struct RpcPresenter {
    events: RpcEventQueue,
    pending_approvals: PendingApprovals,
}

impl RpcPresenter {
    pub fn new(events: RpcEventQueue) -> Self {
        Self {
            events,
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn pending_approvals(&self) -> PendingApprovals {
        Arc::clone(&self.pending_approvals)
    }

    pub fn emit(&self, event: RpcEvent) {
        self.events.push(event);
    }
}

#[async_trait]
impl Presenter for RpcPresenter {
    fn write_output(&self, text: &str) {
        if !text.is_empty() {
            self.emit(RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_welcome(&self, _display: &WelcomeDisplay) {}

    fn print_session_status(&self, display: &SessionStatus) {
        self.emit(RpcEvent::SessionStart {
            session_id: String::new(),
            model: display.model.clone(),
            provider: display.provider.clone(),
        });
    }

    fn print_notice(&self, text: &str) {
        if !text.is_empty() {
            self.emit(RpcEvent::TextChunk {
                content: text.to_string(),
            });
        }
    }

    fn print_user_block(&self, _input: &str) {}

    fn print_token(&self, token: &str) {
        self.emit(RpcEvent::TextChunk {
            content: token.to_string(),
        });
    }

    fn print_thinking_token(&self, token: &str) {
        self.emit(RpcEvent::ReasoningChunk {
            content: token.to_string(),
        });
    }

    fn finish_tool_line(&self, line: ToolLine) {
        self.emit(RpcEvent::ToolCallResult {
            call_id: line.name.clone(),
            tool: line.name,
            output: line.output,
            is_error: line.is_error,
            duration_ms: line.duration_ms.unwrap_or(0),
        });
    }

    fn flush(&self) {}

    fn has_interactive_ui(&self) -> bool {
        true
    }

    async fn request_interaction(&self, prompt: InteractionPrompt) -> Option<InteractionResponse> {
        let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
        let (tx, rx) = oneshot::channel();
        self.pending_approvals.lock().await.insert(approval_id.clone(), tx);

        let arguments = serde_json::json!({
            "body": prompt.body,
            "options": prompt.options,
            "initial_selection": prompt.initial_selection,
            "initial_text": prompt.initial_text,
        });

        self.emit(RpcEvent::ToolApprovalRequest {
            approval_id: approval_id.clone(),
            tool: prompt.title,
            arguments,
            description: Some(prompt.body),
        });
        self.emit(RpcEvent::StatusChanged {
            status: "waiting_approval".to_string(),
        });

        let response = match rx.await {
            Ok(res) => Some(res),
            Err(_) => Some(InteractionResponse::Cancelled),
        };
        self.emit(RpcEvent::StatusChanged {
            status: "busy".to_string(),
        });
        response
    }

    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        self.start_tool_run(name, arguments);
        ActivityToken::default()
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        self.emit(RpcEvent::ToolCallStart {
            call_id: uuid::Uuid::new_v4().to_string(),
            tool: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    /// Clients hear of a call once it starts, with its whole arguments, not while they stream.
    fn preview_tool_run(&self, _name: &str, _arguments: &Value) {}

    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }

    fn print_turn_started(&self, prompt: &str) {
        self.emit(RpcEvent::TurnStart {
            turn_number: 1,
            prompt: prompt.to_string(),
        });
    }

    fn print_turn_completed(&self, status: &str) {
        self.emit(RpcEvent::TurnEnd {
            stop_reason: status.to_string(),
        });
    }
}
