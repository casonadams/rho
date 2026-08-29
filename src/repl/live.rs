use std::time::Duration;

use crossterm::event::{Event, KeyCode};
use tokio::sync::mpsc;

use super::ReplSession;
use super::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use super::input_reader::TerminalInputReader;
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    Activity, BatchDecision, InputAction, InteractionResponder, InteractionResponse, InteractiveState, ModalState,
    OutputEvent, PendingUiBatch, QueuedMessage, TerminalBackend, TerminalController, UiAction, UiEffect, UiEvent,
    map_key,
};
use crate::ui::render::WelcomeDisplay;

type LiveController = TerminalController<crate::ui::interactive::CrosstermBackend>;

const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MAX_PENDING_OUTPUT_BYTES: usize = 16 * 1024;
const SPINNER_FRAME_INTERVALS: usize = 5;

struct PendingModal {
    responder: InteractionResponder,
}

struct LiveBatch {
    ui: PendingUiBatch,
    modal: Option<PendingModal>,
}

impl LiveBatch {
    fn new() -> Self {
        Self {
            ui: PendingUiBatch::new(MAX_PENDING_OUTPUT_BYTES),
            modal: None,
        }
    }

    fn enqueue(&mut self, controller: &mut LiveController, event: UiEvent) -> Result<()> {
        match self.ui.push(event) {
            BatchDecision::Pending => Ok(()),
            BatchDecision::Flush(_) => self.flush(controller, false),
            BatchDecision::Barrier(_, event) => {
                install_interaction(controller, event, &mut self.modal);
                self.flush(controller, true)
            }
        }
    }

    fn flush(&mut self, controller: &mut LiveController, redraw: bool) -> Result<()> {
        let drained = self.ui.drain();
        let mut changed = false;
        if let Some((name, args_summary)) = drained.tool_start {
            controller.start_tool(name, args_summary)?;
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

    fn drain_events(
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

struct LiveIo<'a> {
    controller: &'a mut LiveController,
    events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
    input: &'a mut TerminalInputReader,
}

pub(super) fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

impl ReplSession {
    pub(super) async fn run_live(&mut self) -> Result<()> {
        let mut engine =
            AgentEngine::new(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref()).await?;
        engine.refresh_quota().await;

        let (ui, mut ui_events) = crate::ui::interactive::InteractiveUi::channel();
        self.renderer = TerminalRenderer::with_ui(ui);
        let mut state = InteractiveState::default();
        update_footer(&mut state, self, &engine);
        let mut controller = TerminalController::stdout(state)?;
        let mut input = TerminalInputReader::spawn()?;
        self.renderer.print_welcome(&WelcomeDisplay {
            model: &self.config.model,
            provider: &self.config.provider,
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
        });
        drain_ui_events(&mut controller, &mut ui_events, &mut None)?;

        let mut history = InteractiveHistory::with_file(1000, self.config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?;
        let skill_names =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref())
                .into_iter()
                .map(|skill| skill.metadata.name)
                .collect();
        let completions = CompletionSet::rho(&engine.extension_registry.list_commands(), skill_names);

        loop {
            let message = match controller.state_mut().pop_queued() {
                Some(message) => message,
                None => match read_idle_input(
                    LiveIo {
                        controller: &mut controller,
                        events: &mut ui_events,
                        input: &mut input,
                    },
                    &mut history,
                    &completions,
                )
                .await?
                {
                    Some(message) => message,
                    None => break,
                },
            };
            history
                .record(&message.text)
                .map_err(|error| anyhow::anyhow!("History could not be updated: {error}"))?;
            if self
                .process_live_message(
                    &mut engine,
                    LiveMessage {
                        io: LiveIo {
                            controller: &mut controller,
                            events: &mut ui_events,
                            input: &mut input,
                        },
                        editor: EditorResources {
                            history: &mut history,
                            completions: &completions,
                        },
                        message,
                    },
                )
                .await?
            {
                break;
            }
            update_footer(controller.state_mut(), self, &engine);
            controller.redraw()?;
        }
        input.stop_and_join()?;
        Ok(())
    }

    async fn process_live_message(&mut self, engine: &mut AgentEngine, live: LiveMessage<'_>) -> Result<bool> {
        let controller = live.io.controller;
        let ui_events = live.io.events;
        let input_reader = live.io.input;
        let input = live.message.text.trim();
        let extension_context = engine.extension_context();
        let command_result = if input.starts_with('/') {
            let paused_input = input_reader.pause()?;
            controller.suspend()?;
            let mut command_context = SlashCommandContext {
                config: &mut self.config,
                auth_store: &mut self.auth_store,
                registry: Some(&engine.extension_registry),
                context: Some(&extension_context),
                renderer: &self.renderer,
            };
            let result = SlashCommandHandler::handle(input, &mut command_context).await;
            let controller_result = controller.resume();
            let input_result = paused_input.resume();
            controller_result?;
            input_result?;
            result?
        } else {
            None
        };
        if let Some(result) = command_result {
            match result {
                CommandResult::Exit => return Ok(true),
                CommandResult::ClearContext => {
                    *engine = AgentEngine::new(self.config.clone(), self.auth_store.clone(), None).await?;
                }
                CommandResult::ModelChanged {
                    new_model,
                    new_provider,
                } => {
                    self.config.model = new_model;
                    if let Some(provider) = new_provider {
                        self.config.provider = provider;
                    }
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Login { provider } => {
                    crate::cli::login_provider(provider.as_deref(), &self.config, &mut self.auth_store).await?;
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Logout { provider } => {
                    crate::cli::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                }
                CommandResult::Continue => {}
            }
            drain_ui_events(controller, ui_events, &mut None)?;
            return Ok(false);
        }

        let effective = match engine
            .extension_registry
            .dispatch_input(input, &extension_context)
            .await?
        {
            crate::plugin::InputAction::Continue => input.to_string(),
            crate::plugin::InputAction::Transform(transformed) => transformed,
            crate::plugin::InputAction::Handled { output } => {
                if !output.is_empty() {
                    self.renderer.write_output(&format!("{output}\n"));
                }
                drain_ui_events(controller, ui_events, &mut None)?;
                return Ok(false);
            }
        };
        self.renderer.print_user_block(&effective);
        run_active_turn(
            engine,
            &self.renderer,
            ActiveTurn {
                io: LiveIo {
                    controller,
                    events: ui_events,
                    input: input_reader,
                },
                editor: live.editor,
                prompt: &effective,
            },
        )
        .await?;
        engine.refresh_quota().await;
        Ok(false)
    }
}

struct EditorResources<'a> {
    history: &'a mut InteractiveHistory,
    completions: &'a CompletionSet,
}

struct LiveMessage<'a> {
    io: LiveIo<'a>,
    editor: EditorResources<'a>,
    message: QueuedMessage,
}

async fn read_idle_input(
    live: LiveIo<'_>,
    history: &mut InteractiveHistory,
    completions: &CompletionSet,
) -> Result<Option<QueuedMessage>> {
    let controller = live.controller;
    let ui_events = live.events;
    let input = live.input;
    let mut batch = LiveBatch::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = frame.tick() => batch.flush(controller, false)?,
            event = input.recv() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        batch.flush(controller, false)?;
                        return Err(error.into());
                    }
                    None => {
                        batch.flush(controller, false)?;
                        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
                    }
                };
                if matches!(event, Event::Resize(_, _)) {
                    controller.refresh_size()?;
                    continue;
                }
                let Event::Key(key) = event else { continue };
                if handle_modal_key(controller, key, &mut batch.modal)? {
                    continue;
                }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        let effect = controller.state_mut().apply(action);
                        batch.flush(controller, true)?;
                        if let UiEffect::Queued(message) = effect {
                            controller.state_mut().pop_queued();
                            return Ok(Some(message));
                        }
                    }
                    InputAction::HistoryPrevious => {
                        if navigate_history_previous(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::HistoryNext => {
                        if navigate_history_next(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::Complete => {
                        if apply_completion(controller, completions) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::Cancel => {
                        controller.state_mut().editor_mut().set_text("");
                        controller.redraw()?;
                    }
                    InputAction::ToggleExpandTools => {
                        controller.state_mut().toggle_tools_expanded();
                        batch.flush(controller, true)?;
                    }
                    InputAction::EndOfInput if controller.state().editor().is_empty() => {
                        batch.flush(controller, false)?;
                        return Ok(None);
                    }
                    InputAction::EndOfInput | InputAction::Ignore => {}
                }
            }
            event = ui_events.recv() => {
                if let Some(event) = event {
                    batch.enqueue(controller, event)?;
                }
            }
        }
    }
}

fn navigate_history_previous<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_up(width) {
        return true;
    }
    let Some(value) = history.previous(controller.state().editor().text()) else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

fn navigate_history_next<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_down(width) {
        return true;
    }
    let Some(value) = history.next_entry() else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

fn apply_completion<B: TerminalBackend>(controller: &mut TerminalController<B>, completions: &CompletionSet) -> bool {
    let Some(completion) = completions
        .complete(controller.state().editor().text(), controller.state().editor().cursor())
        .into_iter()
        .next()
    else {
        return false;
    };
    let mut value = controller.state().editor().text().to_string();
    value.replace_range(completion.replacement, &completion.value);
    controller.state_mut().editor_mut().set_text(value);
    true
}

struct ActiveTurn<'a> {
    io: LiveIo<'a>,
    editor: EditorResources<'a>,
    prompt: &'a str,
}

async fn run_active_turn(engine: &AgentEngine, renderer: &TerminalRenderer, active: ActiveTurn<'_>) -> Result<()> {
    let controller = active.io.controller;
    let ui_events = active.io.events;
    let input = active.io.input;
    let history = active.editor.history;
    let completions = active.editor.completions;
    let mut batch = LiveBatch::new();
    let run = engine.run_turn(crate::engine::runner::TurnRequest { prompt: active.prompt }, renderer);
    tokio::pin!(run);
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut frame_count = 0_usize;
    loop {
        tokio::select! {
            biased;
            _ = frame.tick() => {
                frame_count = frame_count.wrapping_add(1);
                let animate = frame_count.is_multiple_of(SPINNER_FRAME_INTERVALS)
                    && controller.state().footer().activity != Activity::Idle;
                if animate {
                    controller.advance_spinner();
                }
                batch.flush(controller, animate)?;
            }
            event = input.recv() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        batch.flush(controller, false)?;
                        return Err(error.into());
                    }
                    None => {
                        batch.flush(controller, false)?;
                        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
                    }
                };
                if matches!(event, Event::Resize(_, _)) {
                    controller.refresh_size()?;
                    continue;
                }
                let Event::Key(key) = event else { continue };
                if handle_modal_key(controller, key, &mut batch.modal)? { continue; }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        controller.state_mut().apply(action);
                        batch.flush(controller, true)?;
                    }
                    InputAction::HistoryPrevious => {
                        if navigate_history_previous(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::HistoryNext => {
                        if navigate_history_next(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::Complete => {
                        if apply_completion(controller, completions) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::ToggleExpandTools => {
                        controller.state_mut().toggle_tools_expanded();
                        batch.flush(controller, true)?;
                    }
                    InputAction::Cancel => {
                        batch.flush(controller, false)?;
                        engine.record_cancellation("operator interrupt").await?;
                        restore_queued_messages(controller);
                        renderer.write_output("\nCanceled.\n");
                        batch.drain_events(controller, ui_events)?;
                        batch.flush(controller, false)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
            result = &mut run => {
                renderer.flush();
                batch.drain_events(controller, ui_events)?;
                batch.flush(controller, false)?;
                if let Err(error) = result {
                    restore_queued_messages(controller);
                    renderer.write_output(&format!("\nError: {error}\n"));
                    batch.drain_events(controller, ui_events)?;
                    batch.flush(controller, false)?;
                }
                return Ok(());
            }
            event = ui_events.recv() => {
                if let Some(event) = event {
                    batch.enqueue(controller, event)?;
                }
            }
        }
    }
}

fn install_interaction(controller: &mut LiveController, event: UiEvent, modal: &mut Option<PendingModal>) {
    let UiEvent::Interaction { prompt, responder } = event else {
        unreachable!("only interaction events create ordered barriers");
    };
    let options = prompt
        .options
        .into_iter()
        .map(|option| crate::ui::interactive::ModalOption {
            label: option.label,
            description: option.description,
        })
        .collect::<Vec<_>>();
    let is_empty_options = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options).with_custom(prompt.allow_custom);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty_options || (prompt.allow_custom && state.options.is_empty()) {
        state.enter_input_mode("answer");
    }
    controller.state_mut().push_modal(state);
    *modal = Some(PendingModal { responder });
}

fn handle_ui_event(controller: &mut LiveController, event: UiEvent, modal: &mut Option<PendingModal>) -> Result<()> {
    match event {
        UiEvent::Output(OutputEvent::Text(text)) => controller.write_output(&text)?,
        UiEvent::Activity(activity) => {
            controller.state_mut().footer_mut().activity = activity;
            controller.redraw()?;
        }
        UiEvent::RunningTool(_) => {}
        UiEvent::ToolStart { name, args_summary } => {
            controller.start_tool(name, args_summary)?;
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

fn handle_modal_key(
    controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>,
    key: crossterm::event::KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<bool> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(false);
    };

    match &active.mode {
        crate::ui::interactive::ModalMode::Input { .. } => match key.code {
            KeyCode::Esc => {
                let has_options = controller.state().active_modal().is_some_and(|m| !m.options.is_empty());
                if has_options {
                    if let Some(modal) = controller.state_mut().active_modal_mut() {
                        modal.exit_input_mode();
                    }
                } else {
                    controller.state_mut().pop_modal();
                    if let Some(pending) = pending.take() {
                        let _ = pending.responder.respond(InteractionResponse::Cancelled);
                    }
                }
            }
            KeyCode::Enter => {
                let custom = controller
                    .state()
                    .active_modal()
                    .map(|m| m.input.text().trim().to_string())
                    .unwrap_or_default();
                controller.state_mut().pop_modal();
                if let Some(pending) = pending.take() {
                    let response = if !custom.is_empty() {
                        InteractionResponse::Custom(custom)
                    } else {
                        InteractionResponse::Cancelled
                    };
                    let _ = pending.responder.respond(response);
                }
            }
            _ => {
                if let InputAction::Edit(action) = map_key(key)
                    && let Some(modal) = controller.state_mut().active_modal_mut()
                {
                    match action {
                        UiAction::Insert(c) => modal.input.insert(c),
                        UiAction::Backspace => modal.input.backspace(),
                        UiAction::Delete => modal.input.delete(),
                        UiAction::MoveLeft => modal.input.move_left(),
                        UiAction::MoveRight => modal.input.move_right(),
                        UiAction::MoveToStart => modal.input.move_to_start(),
                        UiAction::MoveToEnd => modal.input.move_to_end(),
                        _ => {}
                    }
                }
            }
        },
        crate::ui::interactive::ModalMode::Select => match key.code {
            KeyCode::Up | KeyCode::BackTab => controller.state_mut().select_previous_modal_option(),
            KeyCode::Down | KeyCode::Tab => controller.state_mut().select_next_modal_option(),
            KeyCode::Esc => {
                controller.state_mut().pop_modal();
                if let Some(pending) = pending.take() {
                    let _ = pending.responder.respond(InteractionResponse::Cancelled);
                }
            }
            KeyCode::Enter => {
                let selected = controller.state().active_modal().map_or(0, |modal| modal.selected);
                let selected_label = controller
                    .state()
                    .active_modal()
                    .and_then(|m| m.selected_option())
                    .map(|opt| opt.label.clone())
                    .unwrap_or_default();

                let triggers_input = selected_label.contains("with reason")
                    || selected_label.contains("with feedback")
                    || selected_label.contains("custom answer")
                    || selected_label.contains("custom input")
                    || selected_label.contains("Type something")
                    || selected_label.contains("Type a custom")
                    || selected_label == "Deny with reason";

                if triggers_input {
                    let prompt_label = if selected_label.contains("reason") || selected_label.contains("feedback") {
                        "reason"
                    } else {
                        "answer"
                    };
                    if let Some(modal) = controller.state_mut().active_modal_mut() {
                        modal.enter_input_mode(prompt_label);
                    }
                } else {
                    controller.state_mut().pop_modal();
                    if let Some(pending) = pending.take() {
                        let _ = pending.responder.respond(InteractionResponse::Selected(selected));
                    }
                }
            }
            _ => {
                if let InputAction::Edit(UiAction::Insert(c)) = map_key(key) {
                    let allow_custom = controller.state().active_modal().is_some_and(|m| m.allow_custom);
                    if allow_custom && let Some(modal) = controller.state_mut().active_modal_mut() {
                        let prompt_label = if modal.title.contains("Permission") || modal.title.contains("Approve") {
                            "reason"
                        } else {
                            "answer"
                        };
                        modal.enter_input_mode(prompt_label);
                        modal.input.insert(c);
                    }
                }
            }
        },
    }
    controller.redraw()?;
    Ok(true)
}

fn drain_ui_events(
    controller: &mut LiveController,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    while let Ok(event) = events.try_recv() {
        handle_ui_event(controller, event, modal)?;
    }
    Ok(())
}

fn restore_queued_messages(controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>) {
    let mut restored = Vec::new();
    while let Some(message) = controller.state_mut().pop_queued() {
        restored.push(message.text);
    }
    if !restored.is_empty() {
        controller.state_mut().editor_mut().set_text(restored.join("\n\n"));
    }
}

fn update_footer(state: &mut InteractiveState, session: &ReplSession, engine: &AgentEngine) {
    state.footer_mut().activity = Activity::Idle;
    state.footer_mut().model = session.config.model.clone();
    state.footer_mut().context = Some(engine.context_remaining_display());
    state.footer_mut().quota = engine.quota_display();
}

#[cfg(test)]
mod tests {
    use std::{fs, io};

    use super::{live_ui_supported, navigate_history_next, navigate_history_previous};
    use crate::repl::interactive::InteractiveHistory;
    use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};

    struct HistoryTerminal;

    impl TerminalBackend for HistoryTerminal {
        fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
            Ok(())
        }

        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((20, 24))
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn move_up(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_down(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
            Ok(())
        }

        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn write_text(&mut self, _text: &str) -> io::Result<()> {
            Ok(())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn live_ui_requires_both_terminal_streams() {
        assert!(live_ui_supported(true, true));
        assert!(!live_ui_supported(true, false));
        assert!(!live_ui_supported(false, true));
        assert!(!live_ui_supported(false, false));
    }

    #[test]
    fn active_history_navigation_uses_visual_boundaries_and_restores_the_draft() {
        let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
        let mut history = InteractiveHistory::with_file(10, path.clone()).unwrap();
        history.record("older").unwrap();
        history.record("newer\nsecond").unwrap();
        let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
        controller.state_mut().editor_mut().set_text("draft\nline");

        assert!(navigate_history_previous(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "draft\nline");
        assert!(navigate_history_previous(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "newer\nsecond");
        assert!(navigate_history_previous(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "newer\nsecond");
        assert!(navigate_history_previous(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "older");
        assert!(navigate_history_next(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "newer\nsecond");
        assert!(navigate_history_next(&mut controller, &mut history));
        assert_eq!(controller.state().editor().text(), "draft\nline");

        drop(controller);
        drop(history);
        fs::remove_file(path).unwrap();
    }
}
