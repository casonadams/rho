use super::modal::{PendingModal, install_interaction};
use crate::error::Result;
use crate::repl::live::agent_focus::show_agent;
use crate::ui::interactive::{Activity, BatchDecision, PendingUiBatch, PendingUiDrain, TerminalController, UiEvent};
use rho_harness_core::agents::{AgentMonitor, AgentSummary};
use rho_harness_core::config::ApprovalFocus;
use std::time::Duration;
use tokio::sync::mpsc;

pub const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const MAX_PENDING_OUTPUT_BYTES: usize = 16 * 1024;
pub const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(64);

pub struct LiveBatch {
    pub(crate) ui: PendingUiBatch,
    pub(crate) modal: Option<PendingModal>,
    pub(crate) active_turn: bool,
    /// The engine's agents, to tell which one a permission prompt belongs to.
    monitor: Option<AgentMonitor>,
    /// A permission prompt waiting until the agent it belongs to is shown (`None`: the main agent).
    held_interaction: Option<(UiEvent, Option<u64>)>,
}

impl LiveBatch {
    pub fn new() -> Self {
        Self {
            ui: PendingUiBatch::new(MAX_PENDING_OUTPUT_BYTES),
            modal: None,
            active_turn: false,
            monitor: None,
            held_interaction: None,
        }
    }

    pub fn turn() -> Self {
        Self {
            active_turn: true,
            ..Self::new()
        }
    }

    /// Routes permission prompts to the agent that asked, using `monitor` to find it.
    #[must_use]
    pub fn with_monitor(mut self, monitor: AgentMonitor) -> Self {
        self.monitor = Some(monitor);
        self
    }

    pub fn push_event<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<bool> {
        if matches!(event, UiEvent::DismissInteraction) {
            self.modal = None;
            if self.held_interaction.take().is_some() {
                controller.state_mut().footer_mut().awaiting_approval = None;
            } else if controller.state().active_modal().is_some() {
                controller.state_mut().pop_modal();
            }
            return Ok(true);
        }
        match self.ui.push(event) {
            BatchDecision::Pending => Ok(false),
            BatchDecision::Flush(_) => Ok(true),
            BatchDecision::Barrier(_, event) => {
                self.route_interaction(controller, event)?;
                self.flush(controller, true)?;
                Ok(false)
            }
        }
    }

    /// The agent whose permission prompt is open, or `None` for the main agent.
    fn asking_agent(&self) -> Option<AgentSummary> {
        self.monitor
            .as_ref()?
            .summaries()
            .into_iter()
            .filter(|s| s.awaiting_approval)
            .max_by_key(|s| s.id)
    }

    /// Shows a permission prompt over the agent that asked: at once when it is shown, after
    /// switching to it under `ApprovalFocus::Switch`, or once the user views it under `Indicate`.
    fn route_interaction<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        event: UiEvent,
    ) -> Result<()> {
        let asker = self.asking_agent();
        let target = asker.as_ref().map(|s| s.id);
        if target == controller.viewed_agent() {
            install_interaction(controller, event, &mut self.modal);
            return Ok(());
        }
        match controller.approval_focus() {
            ApprovalFocus::Switch => {
                show_agent(controller, self.monitor.as_ref(), target)?;
                install_interaction(controller, event, &mut self.modal);
            }
            ApprovalFocus::Indicate => {
                let label = asker.map_or_else(|| "main agent".to_string(), |s| s.label);
                controller.state_mut().footer_mut().awaiting_approval = Some(label);
                self.held_interaction = Some((event, target));
            }
        }
        Ok(())
    }

    /// Opens a held permission prompt once its agent is the one shown.
    fn release_held_interaction<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
    ) -> bool {
        if !matches!(&self.held_interaction, Some((_, target)) if *target == controller.viewed_agent()) {
            return false;
        }
        let Some((event, _)) = self.held_interaction.take() else {
            return false;
        };
        controller.state_mut().footer_mut().awaiting_approval = None;
        install_interaction(controller, event, &mut self.modal);
        true
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

    pub fn flush<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        controller: &mut TerminalController<B>,
        redraw: bool,
    ) -> Result<()> {
        let released = self.release_held_interaction(controller);
        if self.ui.is_empty() && !redraw && !released {
            return Ok(());
        }
        let mut drained = self.ui.drain();
        let mut changed = released | self.apply_status_updates(controller, &mut drained)?;
        let mut wrote_output = false;
        if controller.viewed_agent().is_some() {
            controller.hold_output(drained);
        } else {
            let (applied, wrote) = controller.apply_output(drained)?;
            changed |= applied;
            wrote_output = wrote;
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
    use crate::ui::interactive::tool_fixture;
    use crate::ui::interactive::{
        Activity, InteractionOption, InteractionPrompt, InteractionResponder, InteractiveState, NullBackend,
        OptionLayout,
    };
    use crate::ui::interactive::{OutputEvent, TranscriptItem};
    use tokio::sync::{mpsc, oneshot};

    #[test]
    fn test_drain_ui_events_empty() {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let mut modal = None;
        drain_ui_events(&mut controller, &mut rx, &mut modal).unwrap();
        assert!(modal.is_none());
    }

    #[test]
    fn test_drain_ui_events_flushes_activity_and_output() {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
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
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
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

    fn queue(batch: &mut LiveBatch, events: Vec<UiEvent>) {
        for event in events {
            let _ = batch.ui.push(event);
        }
    }

    fn flush_events(events: Vec<UiEvent>, redraw: bool) -> (TerminalController<NullBackend>, LiveBatch) {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        let mut batch = LiveBatch::new();
        queue(&mut batch, events);
        batch.flush(&mut controller, redraw).unwrap();
        (controller, batch)
    }

    #[test]
    fn flush_without_pending_work_keeps_activity_idle() {
        for redraw in [false, true] {
            let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
            let mut batch = LiveBatch::new();
            batch.flush(&mut controller, redraw).unwrap();
            assert_eq!(controller.state().footer().activity, Activity::Idle);
        }
    }

    #[test]
    fn flush_applies_activity_per_batch_kind() {
        type Step = (Activity, Activity);
        let cases: [(bool, Vec<Step>); 2] = [
            (true, vec![(Activity::Idle, Activity::Working)]),
            (
                false,
                vec![
                    (Activity::Thinking, Activity::Thinking),
                    (Activity::Idle, Activity::Idle),
                ],
            ),
        ];
        for (turn, steps) in cases {
            let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
            let mut batch = if turn { LiveBatch::turn() } else { LiveBatch::new() };
            for (event, expected) in steps {
                queue(&mut batch, vec![UiEvent::Activity(event)]);
                batch.flush(&mut controller, false).unwrap();
                assert_eq!(controller.state().footer().activity, expected);
            }
        }
    }

    #[test]
    fn flush_sets_and_clears_system_message() {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        let mut batch = LiveBatch::new();
        queue(&mut batch, vec![UiEvent::SystemMessage(Some("syncing".into()))]);
        batch.flush(&mut controller, false).unwrap();
        assert_eq!(controller.state().system_message(), Some("syncing"));
        queue(&mut batch, vec![UiEvent::SystemMessage(None)]);
        batch.flush(&mut controller, false).unwrap();
        assert_eq!(controller.state().system_message(), None);
    }

    #[test]
    fn flush_runs_tool_start_chunks_and_end_without_transcript() {
        let (controller, _) = flush_events(
            vec![
                UiEvent::ToolStart(crate::ui::interactive::ToolStartRequest {
                    name: "bash".into(),
                    args_summary: "cargo test".into(),
                    preview: None,
                }),
                UiEvent::ToolChunk {
                    chunk: "line one".into(),
                },
                UiEvent::ToolEnd,
            ],
            false,
        );
        assert!(controller.state().footer().running_tool.is_none());
    }

    #[test]
    fn flush_drains_tool_end_alongside_tool_transcript() {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        controller
            .start_tool(crate::ui::interactive::ToolStartRequest {
                name: "bash".into(),
                args_summary: "cargo test".into(),
                preview: None,
            })
            .unwrap();
        let mut batch = LiveBatch::new();
        queue(
            &mut batch,
            vec![
                UiEvent::ToolEnd,
                UiEvent::Transcript(tool_fixture("read", serde_json::json!({}), "ok")),
            ],
        );
        batch.flush(&mut controller, false).unwrap();
        assert!(batch.ui.is_empty());
    }

    #[test]
    fn flush_writes_text_and_stream_outputs_and_transcript_items() {
        let (_, batch) = flush_events(
            vec![
                UiEvent::Output(OutputEvent::Text("hello\n".into())),
                UiEvent::Output(OutputEvent::StreamText("partial".into())),
                UiEvent::Transcript(TranscriptItem::Notice("note".into())),
                UiEvent::Transcript(TranscriptItem::AssistantText("answer".into())),
            ],
            false,
        );
        assert!(batch.ui.is_empty());
    }

    #[test]
    fn flush_applies_running_tool_update_with_redraw_request() {
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        let mut batch = LiveBatch::new();
        queue(&mut batch, vec![UiEvent::RunningTool(Some("grep".into()))]);
        batch.flush(&mut controller, true).unwrap();
        assert_eq!(controller.state().footer().running_tool.as_deref(), Some("grep"));
    }
}
