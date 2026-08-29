use crate::ui::interactive::InteractiveUi;

#[derive(Clone, Default)]
pub struct ToolStreamPort {
    ui: Option<InteractiveUi>,
}

impl ToolStreamPort {
    pub fn new(ui: Option<InteractiveUi>) -> Self {
        Self { ui }
    }

    pub fn stream_chunk(&self, chunk: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.tool_chunk(chunk.to_string());
        }
    }
}
