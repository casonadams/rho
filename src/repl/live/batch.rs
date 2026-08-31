use super::modal::{PendingModal, install_interaction};
use crate::error::Result;
use crate::ui::interactive::{BatchDecision, OutputEvent, PendingUiBatch, TerminalController, UiEvent};
use std::time::Duration;
use tokio::sync::mpsc;

pub type LiveController = TerminalController<crate::ui::interactive::CrosstermBackend>;

pub const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const MAX_PENDING_OUTPUT_BYTES: usize = 16 * 1024;
pub const SPINNER_FRAME_INTERVALS: usize = 5;

pub struct LiveBatch {
    pub(crate) ui: PendingUiBatch,
    pub(crate) modal: Option<PendingModal>,
}

impl LiveBatch {
    pub fn new() -> Self {
        Self {
            ui: PendingUiBatch::new(MAX_PENDING_OUTPUT_BYTES),
            modal: None,
        }
    }

    pub fn enqueue(&mut self, controller: &mut LiveController, event: UiEvent) -> Result<()> {
        match self.ui.push(event) {
            BatchDecision::Pending => Ok(()),
            BatchDecision::Flush(_) => self.flush(controller, false),
            BatchDecision::Barrier(_, event) => {
                install_interaction(controller, event, &mut self.modal);
                self.flush(controller, true)
            }
        }
    }

    pub fn flush(&mut self, controller: &mut LiveController, redraw: bool) -> Result<()> {
        let drained = self.ui.drain();
        let mut changed = false;
        if let Some(request) = drained.tool_start {
            controller.start_tool(request)?;
            changed = true;
        }
        for chunk in &drained.tool_chunks {
            controller.append_tool_chunk(chunk)?;
            changed = true;
        }
        if drained.tool_end {
            controller.end_tool()?;
            changed = true;
        }
        for item in drained.transcript_items {
            controller.push_transcript_item(item)?;
            changed = true;
        }
        if let Some(activity) = drained.activity {
            controller.state_mut().footer_mut().activity = activity;
            changed = true;
        }
        if !drained.text.is_empty() {
            controller.write_output(&drained.text)?;
        } else if changed || redraw {
            controller.redraw()?;
        }
        Ok(())
    }

    pub fn drain_events(
        &mut self,
        controller: &mut LiveController,
        events: &mut mpsc::UnboundedReceiver<UiEvent>,
    ) -> Result<()> {
        while let Ok(event) = events.try_recv() {
            self.enqueue(controller, event)?;
        }
        Ok(())
    }
}

pub fn handle_ui_event(
    controller: &mut LiveController,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    match event {
        UiEvent::Output(OutputEvent::Text(text)) => controller.write_output(&text)?,
        UiEvent::Activity(activity) => {
            controller.state_mut().footer_mut().activity = activity;
            controller.redraw()?;
        }
        UiEvent::RunningTool(_) => {}
        UiEvent::Transcript(item) => {
            controller.push_transcript_item(item)?;
        }
        UiEvent::ToolStart(request) => {
            controller.start_tool(request)?;
        }
        UiEvent::ToolChunk { chunk } => {
            controller.append_tool_chunk(&chunk)?;
        }
        UiEvent::ToolEnd => {
            controller.end_tool()?;
        }
        event @ UiEvent::Interaction { .. } => {
            install_interaction(controller, event, modal);
            controller.redraw()?;
        }
    }
    Ok(())
}

pub fn drain_ui_events(
    controller: &mut LiveController,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    while let Ok(event) = events.try_recv() {
        handle_ui_event(controller, event, modal)?;
    }
    Ok(())
}
