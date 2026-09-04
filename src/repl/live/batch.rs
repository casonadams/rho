use super::modal::{PendingModal, install_interaction};
use crate::error::Result;
use crate::ui::interactive::{BatchDecision, PendingUiBatch, TerminalController, UiEvent};
use std::time::Duration;
use tokio::sync::mpsc;

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

    pub fn push_event<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<bool> {
        match self.ui.push(event) {
            BatchDecision::Pending => Ok(false),
            BatchDecision::Flush(_) => Ok(true),
            BatchDecision::Barrier(_, event) => {
                install_interaction(controller, event, &mut self.modal);
                self.flush(controller, true)?;
                Ok(false)
            }
        }
    }

    pub fn enqueue<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<()> {
        if self.push_event(controller, event)? {
            self.flush(controller, false)?;
        }
        Ok(())
    }

    pub fn flush<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        redraw: bool,
    ) -> Result<()> {
        let drained = self.ui.drain();
        let mut changed = false;
        let mut wrote_output = false;

        if let Some(activity) = drained.activity {
            controller.state_mut().footer_mut().activity = activity;
            changed = true;
        }
        if let Some(extra) = drained.extra_status {
            controller.state_mut().footer_mut().extra_status = extra;
            changed = true;
        }
        if let Some(request) = drained.tool_start {
            controller.start_tool(request)?;
            changed = true;
        }
        if !drained.tool_chunks.is_empty() {
            controller.append_tool_chunks(drained.tool_chunks.iter().map(String::as_str))?;
        }

        let has_tool_transcript = drained
            .transcript_items
            .iter()
            .any(|item| matches!(item, crate::ui::interactive::TranscriptItem::Tool(_)));
        if drained.tool_end || has_tool_transcript {
            controller.clear_active_tool();
            if drained.tool_end && drained.transcript_items.is_empty() {
                controller.end_tool()?;
                changed = true;
            }
        } else if let Some(running) = drained.running_tool {
            controller.state_mut().footer_mut().running_tool = running;
            changed = true;
        }

        for item in drained.transcript_items {
            if controller.push_transcript_item(item)? {
                wrote_output = true;
            }
            changed = true;
        }

        if !drained.text.is_empty() {
            controller.write_output(&drained.text)?;
        } else if (changed || redraw) && !wrote_output {
            controller.redraw()?;
        }
        Ok(())
    }

    pub fn drain_events<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        events: &mut mpsc::UnboundedReceiver<UiEvent>,
    ) -> Result<()> {
        let mut needs_flush = false;
        while let Ok(event) = events.try_recv() {
            if self.push_event(controller, event)? {
                needs_flush = true;
            }
        }
        if needs_flush {
            self.flush(controller, false)?;
        }
        Ok(())
    }
}

pub fn drain_ui_events<B: crate::ui::interactive::TerminalBackend>(
    controller: &mut TerminalController<B>,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    let mut batch = LiveBatch::new();
    batch.modal = modal.take();
    let mut needs_flush = false;
    while let Ok(event) = events.try_recv() {
        if batch.push_event(controller, event)? {
            needs_flush = true;
        }
    }
    if needs_flush || !batch.ui.is_empty() {
        batch.flush(controller, false)?;
    }
    *modal = batch.modal;
    Ok(())
}
