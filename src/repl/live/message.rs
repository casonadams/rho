use super::batch::drain_ui_events;
use super::turn::run_active_turn;
use super::{ActiveTurn, LiveIo, LiveMessage};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};

impl ReplSession {
    pub(super) async fn process_live_message(
        &mut self,
        engine: &mut AgentEngine,
        live: LiveMessage<'_>,
    ) -> Result<bool> {
        let controller = live.io.controller;
        let ui_events = live.io.events;
        let input_reader = live.io.input;
        let input = live.message.text.trim();
        let command_result = if input.starts_with('/') {
            let mut command_context = SlashCommandContext {
                config: &mut self.config,
                auth_store: &mut self.auth_store,
                renderer: &self.renderer,
                session_id: Some(&engine.session_manager.session_id),
                session_manager: Some(&engine.session_manager),
            };
            SlashCommandHandler::handle(input, &mut command_context).await?
        } else {
            None
        };
        if let Some(result) = command_result {
            match result {
                CommandResult::Exit => return Ok(true),
                CommandResult::OpenModelSelector => {
                    super::modal::open_model_selector(self, controller);
                    controller.redraw()?;
                }
                CommandResult::OpenSettingsSelector => {
                    super::modal::open_settings_selector(controller);
                    controller.redraw()?;
                }
                CommandResult::ClearContext => {
                    *engine = crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), None).await?;
                }
                CommandResult::ModelChanged {
                    new_model,
                    new_provider,
                } => {
                    self.config.model = new_model.clone();
                    if let Some(provider) = new_provider.as_ref() {
                        self.config.provider = provider.clone();
                    }
                    let _ = rho_harness_core::state::AppState::set_last_model(
                        &self.config.config_dir,
                        &new_model,
                        new_provider.as_deref(),
                    );
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Login { provider } => {
                    crate::cli::login_provider(provider.as_deref(), &self.config, &mut self.auth_store).await?;
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Reload => {
                    *engine = self.reload_engine(engine).await?;
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
                CommandResult::OpenTreeSelector => {
                    let tree = engine.session_manager.load_tree().await?;
                    super::modal::open_tree_selector(&tree, controller);
                    controller.redraw()?;
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
                        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
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
                CommandResult::OpenSessionSelector => {
                    super::modal::open_session_selector(&self.config.sessions_dir, controller);
                    controller.redraw()?;
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
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
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
