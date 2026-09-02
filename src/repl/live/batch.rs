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

    pub fn enqueue<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<()> {
        match self.ui.push(event) {
            BatchDecision::Pending => Ok(()),
            BatchDecision::Flush(_) => self.flush(controller, false),
            BatchDecision::Barrier(_, event) => {
                install_interaction(controller, event, &mut self.modal);
                self.flush(controller, true)
            }
        }
    }

    pub fn flush<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        redraw: bool,
    ) -> Result<()> {
        let drained = self.ui.drain();
        let mut changed = false;
        if let Some(request) = drained.tool_start {
            controller.start_tool(request)?;
            changed = true;
        }
        if !drained.tool_chunks.is_empty() {
            controller.append_tool_chunks(drained.tool_chunks.iter().map(String::as_str))?;
        }
        if drained.tool_end {
            if drained.transcript_items.is_empty() {
                controller.end_tool()?;
                changed = true;
            } else {
                controller.clear_active_tool();
            }
        }
        for item in drained.transcript_items {
            controller.push_transcript_item(item)?;
            changed = true;
        }
        if let Some(activity) = drained.activity {
            controller.state_mut().footer_mut().activity = activity;
            changed = true;
        }
        if let Some(extra) = drained.extra_status {
            controller.state_mut().footer_mut().extra_status = extra;
            changed = true;
        }
        if !drained.text.is_empty() {
            controller.write_output(&drained.text)?;
        } else if changed || redraw {
            controller.redraw()?;
        }
        Ok(())
    }

    pub fn drain_events<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        events: &mut mpsc::UnboundedReceiver<UiEvent>,
    ) -> Result<()> {
        while let Ok(event) = events.try_recv() {
            self.enqueue(controller, event)?;
        }
        Ok(())
    }
}

pub fn handle_ui_event<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
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
        UiEvent::ExtraStatus(status) => {
            controller.state_mut().footer_mut().extra_status = status;
            controller.redraw()?;
        }
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

pub fn drain_ui_events<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    while let Ok(event) = events.try_recv() {
        handle_ui_event(controller, event, modal)?;
    }
    Ok(())
}
