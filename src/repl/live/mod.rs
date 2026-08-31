pub mod batch;
pub mod modal;
pub mod navigation;

#[cfg(test)]
mod tests;

pub use batch::LiveController;

use batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS, drain_ui_events};
use modal::handle_modal_key;
use navigation::{
    apply_completion, navigate_history_next, navigate_history_previous, restore_queued_messages, update_footer,
};

use crossterm::event::Event;
use tokio::sync::mpsc;

use super::ReplSession;
use super::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use super::input_reader::TerminalInputReader;
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    Activity, InputAction, InteractiveState, QueuedMessage, TerminalController, UiEffect, UiEvent, map_key,
};
use crate::ui::render::WelcomeDisplay;

pub struct LiveIo<'a> {
    pub controller: &'a mut LiveController,
    pub events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
    pub input: &'a mut TerminalInputReader,
}

pub struct EditorResources<'a> {
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
}

pub struct LiveMessage<'a> {
    pub io: LiveIo<'a>,
    pub editor: EditorResources<'a>,
    pub message: QueuedMessage,
}

pub struct ActiveTurn<'a> {
    pub io: LiveIo<'a>,
    pub editor: EditorResources<'a>,
    pub prompt: &'a str,
}

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

impl ReplSession {
    pub(super) async fn run_live(&mut self) -> Result<()> {
        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        engine.refresh_quota().await;

        let (ui, mut ui_events) = crate::ui::interactive::InteractiveUi::channel();
        self.renderer = TerminalRenderer::with_ui(ui);
        let mut state = InteractiveState::default();
        update_footer(&mut state, self, &engine);
        let mut controller = TerminalController::stdout(state)?;
        let mut input = TerminalInputReader::spawn()?;
        self.renderer.print_welcome(&WelcomeDisplay {
            model: self.config.model.clone(),
            provider: self.config.provider.clone(),
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
        let assembly = crate::platform::active_tools(&self.config, &std::env::current_dir()?).await?;
        self.commands = assembly.commands;
        let ext_cmds: Vec<(&str, &str)> = self.commands.keys().map(|k| (k.as_str(), "")).collect();
        let prompt_templates = rho_core::prompts::discover_prompt_templates(
            Some(&self.config.config_dir),
            std::env::current_dir().ok().as_deref(),
        )
        .into_iter()
        .map(|t| t.metadata.name)
        .collect::<Vec<_>>();
        let completions = CompletionSet::rho(&ext_cmds, skill_names, prompt_templates);

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
        let command_result = if input.starts_with('/') {
            let paused_input = input_reader.pause()?;
            controller.suspend()?;
            let mut command_context = SlashCommandContext {
                config: &mut self.config,
                auth_store: &mut self.auth_store,
                renderer: &self.renderer,
                commands: Some(&self.commands),
                session_id: Some(&engine.session_manager.session_id),
                session_manager: Some(&engine.session_manager),
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
                    *engine = crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), None).await?;
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
                CommandResult::Compact { .. } => {
                    let session_id = engine.session_manager.session_id.clone();
                    self.renderer.print_notice("  [Compacting conversation context...]\n");
                    let memory = crate::session::context::context_memory(
                        engine.session_manager.clone(),
                        1,
                        self.config.compaction_max_bytes,
                    );
                    let _ = memory.load(&session_id).await;
                    self.renderer.print_notice("  [Context compaction completed]\n");
                }
                CommandResult::Tree => {
                    let tree = engine.session_manager.load_tree().await?;
                    let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
                    self.renderer.print_notice(&format!(
                        "\nConversation Tree (Session: {}):\n{rendered}\n",
                        engine.session_manager.session_id
                    ));
                }
                CommandResult::SwitchBranch { leaf_id } => {
                    let old_leaf = engine.session_manager.active_leaf_id().await?.unwrap_or_default();
                    let tree = engine.session_manager.load_tree().await?;
                    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
                    let has_assistant = abandoned
                        .iter()
                        .any(|n| n.kind == rho_core::session::TreeNodeKind::AssistantTurn);
                    if has_assistant
                        && self.renderer.has_interactive_ui()
                        && let Ok(true) =
                            inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
                                .with_default(true)
                                .prompt()
                    {
                        let summary_text = abandoned
                            .iter()
                            .map(|n| format!("{:?}", n.messages))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let _ = engine
                            .session_manager
                            .append_branch_summary(&summary_text, &old_leaf)
                            .await;
                    }
                    let _ = engine.session_manager.switch_branch(Some(leaf_id.clone())).await?;
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                    self.renderer
                        .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
                }
                CommandResult::ForkSession { turn_or_node_id } => {
                    let forked = engine
                        .session_manager
                        .fork_session(&self.config.sessions_dir, turn_or_node_id.as_deref())
                        .await?;
                    self.renderer
                        .print_notice(&format!("  [Forked session into {}]\n", forked.session_id));
                }
                CommandResult::CloneSession => {
                    let cloned = engine.session_manager.clone_session(&self.config.sessions_dir).await?;
                    self.renderer
                        .print_notice(&format!("  [Cloned session into {}]\n", cloned.session_id));
                }
                CommandResult::ResumeSession { session_id } => {
                    *engine =
                        crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), Some(&session_id))
                            .await?;
                    self.renderer
                        .print_notice(&format!("  [Resumed session {session_id}]\n"));
                }
                CommandResult::NameSession { name } => {
                    engine.session_manager.set_session_name(&name).await?;
                    self.renderer.print_notice(&format!("  [Named session: \"{name}\"]\n"));
                }
                CommandResult::ExpandedPrompt { text } => {
                    self.renderer.print_notice("  [Expanded template]\n");
                    drain_ui_events(controller, ui_events, &mut None)?;
                    let effective = text;
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
                    return Ok(false);
                }
                CommandResult::Rewind { turn } => {
                    let retained_count = engine.session_manager.rewind_to_turn(turn).await?;
                    self.renderer.print_notice(&format!(
                        "  [Rewound context to Turn {turn} ({retained_count} messages retained)]\n"
                    ));
                }
                CommandResult::Logout { provider } => {
                    crate::cli::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                }
                CommandResult::Continue => {}
            }
            drain_ui_events(controller, ui_events, &mut None)?;
            return Ok(false);
        }

        if let Some(cmd) = input.strip_prefix("!!") {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.renderer
                    .print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
                #[cfg(unix)]
                let out = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await;
                #[cfg(windows)]
                let out = tokio::process::Command::new("cmd.exe")
                    .arg("/c")
                    .arg(cmd)
                    .output()
                    .await;
                match out {
                    Ok(res) => {
                        let stdout = String::from_utf8_lossy(&res.stdout);
                        let stderr = String::from_utf8_lossy(&res.stderr);
                        if !stdout.is_empty() {
                            self.renderer.write_output(&stdout);
                        }
                        if !stderr.is_empty() {
                            self.renderer.write_output(&stderr);
                        }
                    }
                    Err(e) => {
                        self.renderer
                            .print_notice(&format!("  Command execution failed: {e}\n"));
                    }
                }
                drain_ui_events(controller, ui_events, &mut None)?;
                return Ok(false);
            }
        }

        let effective = if let Some(cmd) = input.strip_prefix('!') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.renderer
                    .print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
                #[cfg(unix)]
                let out = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await;
                #[cfg(windows)]
                let out = tokio::process::Command::new("cmd.exe")
                    .arg("/c")
                    .arg(cmd)
                    .output()
                    .await;
                match out {
                    Ok(res) => {
                        let stdout = String::from_utf8_lossy(&res.stdout);
                        let stderr = String::from_utf8_lossy(&res.stderr);
                        if !stdout.is_empty() {
                            self.renderer.write_output(&stdout);
                        }
                        if !stderr.is_empty() {
                            self.renderer.write_output(&stderr);
                        }
                        format!(
                            "Executed local shell command: `{cmd}`\n\nOutput:\n```\n{}{}\n```",
                            stdout, stderr
                        )
                    }
                    Err(e) => {
                        self.renderer
                            .print_notice(&format!("  Command execution failed: {e}\n"));
                        format!("Failed to execute local shell command `{cmd}`: {e}")
                    }
                }
            } else {
                input.to_string()
            }
        } else {
            input.to_string()
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
                        controller.toggle_tools_expanded()?;
                    }
                    InputAction::ExternalEditor => {
                        let current_text = controller.state().editor().text().to_string();
                        let temp_file =
                            std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
                        let _ = std::fs::write(&temp_file, &current_text);
                        let editor = std::env::var("VISUAL")
                            .or_else(|_| std::env::var("EDITOR"))
                            .unwrap_or_else(|_| "nano".to_string());
                        let paused = input.pause()?;
                        controller.suspend()?;
                        let status = std::process::Command::new(&editor)
                            .arg(&temp_file)
                            .status();
                        let controller_res = controller.resume();
                        let input_res = paused.resume();
                        controller_res?;
                        input_res?;
                        if status.is_ok()
                            && let Ok(edited_text) = std::fs::read_to_string(&temp_file)
                        {
                            controller
                                .state_mut()
                                .editor_mut()
                                .set_text(edited_text.trim_end());
                        }
                        let _ = std::fs::remove_file(temp_file);
                        batch.flush(controller, true)?;
                    }
                    InputAction::DequeueQueued => {
                        let queued = controller.state_mut().dequeue_all();
                        if !queued.is_empty() {
                            let text = queued
                                .into_iter()
                                .map(|m| m.text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            controller.state_mut().editor_mut().set_text(&text);
                            batch.flush(controller, true)?;
                        }
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

async fn run_active_turn(engine: &AgentEngine, renderer: &TerminalRenderer, turn: ActiveTurn<'_>) -> Result<()> {
    let ActiveTurn {
        io: LiveIo {
            controller,
            events: ui_events,
            input: input_reader,
        },
        editor: EditorResources { history, completions },
        prompt,
    } = turn;

    let request = crate::engine::runner::TurnRequest::new(prompt);
    let mut batch = LiveBatch::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut spinner_tick = 0_usize;
    let mut run = std::pin::pin!(engine.run_turn(request, std::sync::Arc::new(renderer.clone())));

    loop {
        tokio::select! {
            biased;
            _ = frame.tick() => {
                spinner_tick += 1;
                let spinner_advanced = if spinner_tick >= SPINNER_FRAME_INTERVALS {
                    spinner_tick = 0;
                    controller.advance_spinner();
                    !matches!(controller.state().footer().activity, Activity::Idle)
                } else {
                    false
                };
                batch.flush(controller, spinner_advanced)?;
            }
            event = input_reader.recv() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        batch.flush(controller, false)?;
                        return Err(error.into());
                    }
                    None => continue,
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
                        controller.toggle_tools_expanded()?;
                    }
                    InputAction::DequeueQueued => {
                        let queued = controller.state_mut().dequeue_all();
                        if !queued.is_empty() {
                            let text = queued
                                .into_iter()
                                .map(|m| m.text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            controller.state_mut().editor_mut().set_text(&text);
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::Cancel => {
                        batch.flush(controller, false)?;
                        engine.record_cancellation("operator interrupt").await?;
                        restore_queued_messages(controller);
                        renderer.print_notice("\nCanceled.\n");
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
                    renderer.print_notice(&format!("\nError: {error}\n"));
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
