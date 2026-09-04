//! Interactive stream sink bridge to InteractiveUi.

use crate::ui::interactive::InteractiveUi;
use rho_harness_core::presentation::stream::ToolStreamSink;

pub struct InteractiveStreamSink(pub Option<InteractiveUi>);

impl ToolStreamSink for InteractiveStreamSink {
    fn tool_chunk(&self, chunk: String) {
        if let Some(ui) = &self.0 {
            let _ = ui.tool_chunk(chunk);
        }
    }
}
