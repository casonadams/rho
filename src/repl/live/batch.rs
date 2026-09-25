use super::modal::{PendingModal, install_interaction};
use crate::error::Result;
use crate::ui::interactive::{Activity, BatchDecision, PendingUiBatch, PendingUiDrain, TerminalController, UiEvent};
use std::time::Duration;
use tokio::sync::mpsc;

pub const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const MAX_PENDING_OUTPUT_BYTES: usize = 16 * 1024;
pub const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(64);

pub struct LiveBatch {
    pub(crate) ui: PendingUiBatch,
    pub(crate) modal: Option<PendingModal>,
    pub(crate) active_turn: bool,
}

impl LiveBatch {
    pub fn new() -> Self {
        Self {
            ui: PendingUiBatch::new(MAX_PENDING_OUTPUT_BYTES),
            modal: None,
            active_turn: false,
        }
    }

    pub fn turn() -> Self {
        Self {
            ui: PendingUiBatch::new(MAX_PENDING_OUTPUT_BYTES),
            modal: None,
            active_turn: true,
        }
    }

    pub fn push_event<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<bool> {
        if matches!(event, UiEvent::DismissInteraction) {
            self.modal = None;
            if controller.state().active_modal().is_some() {
                controller.state_mut().pop_modal();
            }
            return Ok(true);
        }
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

    fn apply_status_updates<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        drained: &mut PendingUiDrain,
    ) -> Result<bool> {
        let mut changed = false;
        if let Some(activity) = drained.activity.take() {
            let activity = if self.active_turn && matches!(activity, Activity::Idle) {
                Activity::Working
            } else {
                activity
            };
            if controller.state().footer().activity != activity {
                controller.state_mut().footer_mut().activity = activity;
                changed = true;
            }
        }
        if let Some(extra) = drained.extra_status.take() {
            controller.state_mut().footer_mut().extra_status = extra;
            changed = true;
        }
        if let Some(system_msg) = drained.system_message.take() {
            if let Some(msg) = system_msg {
                controller.set_system_message(msg);
            } else {
                controller.clear_system_message();
            }
            changed = true;
        }
        Ok(changed)
    }

    fn apply_tool_updates<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        drained: &mut PendingUiDrain,
    ) -> Result<bool> {
        let mut changed = false;
        if let Some(request) = drained.tool_start.take() {
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
        if drained.tool_end && !has_tool_transcript {
            controller.end_tool()?;
            changed = true;
        } else if !has_tool_transcript && let Some(running) = drained.running_tool.take() {
            controller.state_mut().footer_mut().running_tool = running;
            changed = true;
        }
        Ok(changed)
    }

    pub fn flush<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        redraw: bool,
    ) -> Result<()> {
        if self.ui.is_empty() && !redraw {
            return Ok(());
        }
        let mut drained = self.ui.drain();
        let mut changed = self.apply_status_updates(controller, &mut drained)?;
        changed |= self.apply_tool_updates(controller, &mut drained)?;
        let mut wrote_output = false;

        for output in drained.outputs {
            match output {
                crate::ui::interactive::OutputEvent::Text(text) => {
                    controller.write_output(&text)?;
                    wrote_output = true;
                }
                crate::ui::interactive::OutputEvent::StreamText(text) => {
                    controller.write_stream_output(&text)?;
                    wrote_output = true;
                }
            }
        }

        for item in drained.transcript_items {
            if controller.push_transcript_item(item)? {
                wrote_output = true;
            }
            changed = true;
        }

        if (changed || redraw) && !wrote_output {
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
        if needs_flush || !self.ui.is_empty() {
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
    let result = batch.drain_events(controller, events);
    *modal = batch.modal;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{
        Activity, InteractionOption, InteractionPrompt, InteractionResponder, InteractiveState, OptionLayout,
        TerminalBackend,
    };
    use std::io;
    use tokio::sync::{mpsc, oneshot};

    struct MockBackend;

    impl TerminalBackend for MockBackend {
        fn set_raw_mode(&mut self, _: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_drain_ui_events_empty() {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let mut modal = None;
        drain_ui_events(&mut controller, &mut rx, &mut modal).unwrap();
        assert!(modal.is_none());
    }

    #[test]
    fn test_drain_ui_events_flushes_activity_and_output() {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(UiEvent::Activity(Activity::Working)).unwrap();
        tx.send(UiEvent::RunningTool(Some("read".into()))).unwrap();
        let mut modal = None;
        drain_ui_events(&mut controller, &mut rx, &mut modal).unwrap();
        assert_eq!(controller.state().footer().activity, Activity::Working);
        assert_eq!(controller.state().footer().running_tool.as_deref(), Some("read"));
    }

    #[test]
    fn test_drain_ui_events_dismiss_interaction_clears_modal() {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let (oneshot_tx, _oneshot_rx) = oneshot::channel();
        let mut modal = None;
        install_interaction(
            &mut controller,
            UiEvent::Interaction {
                prompt: InteractionPrompt {
                    title: "Test".into(),
                    body: "Confirm?".into(),
                    options: vec![InteractionOption {
                        label: "Yes".into(),
                        description: None,
                        input: None,
                    }],
                    initial_selection: 0,
                    allow_custom: false,
                    initial_text: None,
                    option_layout: OptionLayout::Vertical,
                },
                responder: InteractionResponder { responder: oneshot_tx },
            },
            &mut modal,
        );
        assert!(modal.is_some());
        assert!(controller.state().active_modal().is_some());

        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(UiEvent::DismissInteraction).unwrap();
        drain_ui_events(&mut controller, &mut rx, &mut modal).unwrap();
        assert!(modal.is_none());
        assert!(controller.state().active_modal().is_none());
    }
}
