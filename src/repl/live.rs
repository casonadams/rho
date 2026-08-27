use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use tokio::sync::mpsc;

use super::ReplSession;
use super::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    Activity, InputAction, InteractionResponder, InteractionResponse, InteractiveState, ModalState, OutputEvent,
    QueuedMessage, TerminalController, UiAction, UiEffect, UiEvent, map_key,
};
use crate::ui::render::WelcomeDisplay;

type LiveController = TerminalController<crate::ui::interactive::CrosstermBackend>;

struct PendingModal {
    responder: InteractionResponder,
    allow_custom: bool,
}

struct LiveIo<'a> {
    controller: &'a mut LiveController,
    events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
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
        self.renderer.print_welcome(&WelcomeDisplay {
            model: &self.config.model,
            provider: &self.config.provider,
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
        });
        drain_ui_events(&mut controller, &mut ui_events, &mut None)?;

        let mut history = InteractiveHistory::with_file(1000, self.config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?;
        let completions = CompletionSet::rho(&engine.extension_registry.list_commands());

        loop {
            let message = match controller.state_mut().pop_queued() {
                Some(message) => message,
                None => match read_idle_input(
                    LiveIo {
                        controller: &mut controller,
                        events: &mut ui_events,
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
        Ok(())
    }

    async fn process_live_message(&mut self, engine: &mut AgentEngine, live: LiveMessage<'_>) -> Result<bool> {
        let controller = live.io.controller;
        let ui_events = live.io.events;
        let input = live.message.text.trim();
        let extension_context = engine.extension_context();
        let command_result = if input.starts_with('/') {
            controller.suspend()?;
            let mut command_context = SlashCommandContext {
                config: &mut self.config,
                auth_store: &mut self.auth_store,
                registry: Some(&engine.extension_registry),
                context: Some(&extension_context),
                renderer: &self.renderer,
            };
            let result = SlashCommandHandler::handle(input, &mut command_context).await;
            controller.resume()?;
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
                },
                prompt: &effective,
            },
        )
        .await?;
        engine.refresh_quota().await;
        Ok(false)
    }
}

struct LiveMessage<'a> {
    io: LiveIo<'a>,
    message: QueuedMessage,
}

async fn read_idle_input(
    live: LiveIo<'_>,
    history: &mut InteractiveHistory,
    completions: &CompletionSet,
) -> Result<Option<QueuedMessage>> {
    let controller = live.controller;
    let ui_events = live.events;
    let mut modal = None;
    loop {
        tokio::select! {
            event = ui_events.recv() => {
                if let Some(event) = event {
                    handle_ui_event(controller, event, &mut modal)?;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                controller.refresh_size()?;
                if !event::poll(Duration::ZERO)? {
                    continue;
                }
                let Event::Key(key) = event::read()? else { continue };
                if handle_modal_key(controller, key, &mut modal)? {
                    continue;
                }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        let effect = controller.state_mut().apply(action);
                        controller.redraw()?;
                        if let UiEffect::Queued(message) = effect {
                            controller.state_mut().pop_queued();
                            return Ok(Some(message));
                        }
                    }
                    InputAction::HistoryPrevious => {
                        if let Some(value) = history.previous(controller.state().editor().text()) {
                            controller.state_mut().editor_mut().set_text(value);
                            controller.redraw()?;
                        }
                    }
                    InputAction::HistoryNext => {
                        if let Some(value) = history.next_entry() {
                            controller.state_mut().editor_mut().set_text(value);
                            controller.redraw()?;
                        }
                    }
                    InputAction::Complete => {
                        if let Some(completion) = completions
                            .complete(controller.state().editor().text(), controller.state().editor().cursor())
                            .into_iter()
                            .next()
                        {
                            let mut value = controller.state().editor().text().to_string();
                            value.replace_range(completion.replacement, &completion.value);
                            controller.state_mut().editor_mut().set_text(value);
                            controller.redraw()?;
                        }
                    }
                    InputAction::Cancel => {
                        controller.state_mut().editor_mut().set_text("");
                        controller.redraw()?;
                    }
                    InputAction::EndOfInput if controller.state().editor().is_empty() => return Ok(None),
                    InputAction::EndOfInput | InputAction::Ignore => {}
                }
            }
        }
    }
}

struct ActiveTurn<'a> {
    io: LiveIo<'a>,
    prompt: &'a str,
}

async fn run_active_turn(engine: &AgentEngine, renderer: &TerminalRenderer, active: ActiveTurn<'_>) -> Result<()> {
    let controller = active.io.controller;
    let ui_events = active.io.events;
    let mut modal = None;
    let run = engine.run_turn(crate::engine::runner::TurnRequest { prompt: active.prompt }, renderer);
    tokio::pin!(run);
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(80));
    spinner_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut input_tick = tokio::time::interval(Duration::from_millis(20));
    input_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut run => {
                renderer.flush();
                if let Err(error) = result {
                    restore_queued_messages(controller);
                    renderer.write_output(&format!("\nError: {error}\n"));
                }
                drain_ui_events(controller, ui_events, &mut modal)?;
                return Ok(());
            }
            event = ui_events.recv() => {
                if let Some(event) = event {
                    handle_ui_event(controller, event, &mut modal)?;
                }
            }
            _ = spinner_tick.tick() => controller.tick()?,
            _ = input_tick.tick() => {
                controller.refresh_size()?;
                if !event::poll(Duration::ZERO)? { continue; }
                let Event::Key(key) = event::read()? else { continue };
                if handle_modal_key(controller, key, &mut modal)? { continue; }
                match map_key(key) {
                    InputAction::Edit(action @ UiAction::Submit(_)) => {
                        controller.state_mut().apply(action);
                        controller.redraw()?;
                    }
                    InputAction::Edit(action) => {
                        controller.state_mut().apply(action);
                        controller.redraw()?;
                    }
                    InputAction::HistoryPrevious => {
                        let width = usize::from(crossterm::terminal::size()?.0).max(1);
                        controller.state_mut().editor_mut().move_up(width);
                        controller.redraw()?;
                    }
                    InputAction::HistoryNext => {
                        let width = usize::from(crossterm::terminal::size()?.0).max(1);
                        controller.state_mut().editor_mut().move_down(width);
                        controller.redraw()?;
                    }
                    InputAction::Cancel => {
                        engine.record_cancellation("operator interrupt").await?;
                        restore_queued_messages(controller);
                        renderer.write_output("\nCanceled.\n");
                        drain_ui_events(controller, ui_events, &mut modal)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn handle_ui_event(
    controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) -> Result<()> {
    match event {
        UiEvent::Output(OutputEvent::Text(text)) => controller.write_output(&text)?,
        UiEvent::Activity(activity) => {
            controller.state_mut().footer_mut().activity = activity;
            controller.redraw()?;
        }
        UiEvent::Interaction { prompt, responder } => {
            let options = prompt
                .options
                .into_iter()
                .map(|option| match option.description {
                    Some(description) => format!("{} - {description}", option.label),
                    None => option.label,
                })
                .collect();
            let mut state = ModalState::new(prompt.title, prompt.body, options);
            state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
            controller.state_mut().push_modal(state);
            *modal = Some(PendingModal {
                responder,
                allow_custom: prompt.allow_custom,
            });
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
    if pending.is_none() {
        return Ok(false);
    }
    match key.code {
        KeyCode::Up => controller.state_mut().select_previous_modal_option(),
        KeyCode::Down => controller.state_mut().select_next_modal_option(),
        KeyCode::Esc => {
            controller.state_mut().pop_modal();
            if let Some(pending) = pending.take() {
                let _ = pending.responder.respond(InteractionResponse::Cancelled);
            }
        }
        KeyCode::Enter => {
            let custom = controller.state().editor().text().trim().to_string();
            let selected = controller.state().active_modal().map_or(0, |modal| modal.selected);
            controller.state_mut().pop_modal();
            if let Some(pending) = pending.take() {
                let response = if pending.allow_custom && !custom.is_empty() {
                    InteractionResponse::Custom(custom)
                } else {
                    InteractionResponse::Selected(selected)
                };
                let _ = pending.responder.respond(response);
            }
        }
        _ => {
            if let InputAction::Edit(action) = map_key(key) {
                controller.state_mut().apply(action);
            }
        }
    }
    controller.redraw()?;
    Ok(true)
}

fn drain_ui_events(
    controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>,
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
    use super::live_ui_supported;

    #[test]
    fn live_ui_requires_both_terminal_streams() {
        assert!(live_ui_supported(true, true));
        assert!(!live_ui_supported(true, false));
        assert!(!live_ui_supported(false, true));
        assert!(!live_ui_supported(false, false));
    }
}
