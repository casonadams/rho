//! Neutral tool-outcome streaming port handed to tool dispatch contexts.

use std::sync::Arc;

/// Receives incremental tool output chunks.
pub trait ToolStreamSink: Send + Sync {
    fn tool_chunk(&self, chunk: String);
}

#[derive(Clone, Default)]
pub struct ToolStreamPort {
    sink: Option<Arc<dyn ToolStreamSink>>,
}

impl ToolStreamPort {
    pub fn new(sink: Option<Arc<dyn ToolStreamSink>>) -> Self {
        Self { sink }
    }

    pub fn stream_chunk(&self, chunk: &str) {
        if let Some(sink) = &self.sink {
            sink.tool_chunk(chunk.to_string());
        }
    }
}
