use super::super::sink::TerminalApprovalSink;
use crate::engine::provider::host_loop::NeutralTurnObserver;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) struct TurnObserver {
    pub(crate) sink: Arc<TerminalApprovalSink>,
    pub(crate) tool_calls: AtomicUsize,
}

impl TurnObserver {
    pub(crate) fn new(sink: Arc<TerminalApprovalSink>) -> Self {
        Self {
            sink,
            tool_calls: AtomicUsize::new(0),
        }
    }
}

impl NeutralTurnObserver for TurnObserver {
    fn text_delta(&self, text: &str) {
        self.sink.emit_text(text);
    }

    fn tool_call(&self, _call: &crate::engine::provider::host_loop::NeutralToolCall) {
        self.tool_calls.fetch_add(1, Ordering::Relaxed);
        self.sink.resume_model_spinner();
    }
}
