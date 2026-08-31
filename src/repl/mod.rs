pub mod commands;
pub mod coordinator;
mod input_reader;
pub mod interactive;
mod live;
mod prompt;

pub use prompt::SimplePrompt;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use crate::repl::interactive::CompletionSet;
use crate::ui::TerminalRenderer;
use crate::ui::render::{SessionStatus, WelcomeDisplay};
use crossterm::QueueableCommand;
use crossterm::cursor::{MoveToColumn, MoveUp};
use crossterm::terminal::{Clear, ClearType};
use crossterm::tty::IsTty;
use reedline::{
    ColumnarMenu, Completer, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Span, Suggestion, default_emacs_keybindings,
};
use rho_sdk::contract::CommandCapability;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

fn submitted_input_rows(input: &str, terminal_width: usize) -> u16 {
    let width = terminal_width.max(1);
    input.lines().fold(0_u16, |rows, line| {
        let occupied = UnicodeWidthStr::width(line).saturating_add(2);
        rows.saturating_add((occupied / width + 1).try_into().unwrap_or(u16::MAX))
    })
}

fn clear_submitted_input(input: &str) {
    let mut stdout = std::io::stdout();
    if !stdout.is_tty() {
        return;
    }
    let width = crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns))
        .unwrap_or(80);
    let rows = submitted_input_rows(input, width);
    let _ = stdout
        .queue(MoveUp(rows))
        .and_then(|stream| stream.queue(MoveToColumn(0)))
        .and_then(|stream| stream.queue(Clear(ClearType::FromCursorDown)))
        .and_then(Write::flush);
}

#[derive(Clone)]
pub struct RhoCompleter {
    completions: CompletionSet,
}

impl RhoCompleter {
    pub fn new(extension_commands: &[(&str, &str)], skill_names: Vec<String>, prompt_templates: Vec<String>) -> Self {
        Self {
            completions: CompletionSet::rho(extension_commands, skill_names, prompt_templates),
        }
    }
}

impl Completer for RhoCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.completions
            .complete(line, pos)
            .into_iter()
            .map(|completion| Suggestion {
                value: completion.value,
                description: None,
                style: None,
                extra: None,
                span: Span::new(completion.replacement.start, completion.replacement.end),
                append_whitespace: true,
            })
            .collect()
    }
}

pub struct ReplSession {
    pub config: Config,
    pub auth_store: AuthStore,
    pub renderer: TerminalRenderer,
    pub resume_id: Option<String>,
    pub commands: BTreeMap<String, Arc<dyn CommandCapability>>,
}

impl ReplSession {
    pub fn new(config: Config, auth_store: AuthStore, resume_id: Option<String>) -> Self {
        Self {
            config,
            auth_store,
            renderer: TerminalRenderer::default(),
            resume_id,
            commands: BTreeMap::new(),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        if live::live_ui_supported(std::io::stdin().is_tty(), std::io::stdout().is_tty()) {
            self.run_live().await
        } else {
            self.run_legacy().await
        }
    }

    async fn run_legacy(&mut self) -> Result<()> {
        self.renderer.print_welcome(&WelcomeDisplay {
            model: self.config.model.clone(),
            provider: self.config.provider.clone(),
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
        });

        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        engine.refresh_quota().await;

        let assembly = crate::platform::active_tools(&self.config, &std::env::current_dir()?).await?;
        self.commands = assembly.commands;
        let ext_cmds: Vec<(&str, &str)> = self.commands.keys().map(|k| (k.as_str(), "")).collect();
        let history_file = self.config.config_dir.join("history.txt");
        let history =
            Box::new(FileBackedHistory::with_file(1000, history_file).unwrap_or_else(|_| FileBackedHistory::default()));
        let skill_names =
            crate::skills::resolved_skills(Some(&self.config.config_dir), Some(&std::env::current_dir()?))
                .into_iter()
                .map(|skill| skill.metadata.name)
                .collect();
        let prompt_templates = rho_core::prompts::discover_prompt_templates(
            Some(&self.config.config_dir),
            std::env::current_dir().ok().as_deref(),
        )
        .into_iter()
        .map(|t| t.metadata.name)
        .collect::<Vec<_>>();
        let completer = RhoCompleter::new(&ext_cmds, skill_names, prompt_templates);
        let completion_menu = Box::new(ColumnarMenu::default().with_name("slash_commands"));
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("slash_commands".to_string()),
                ReedlineEvent::MenuNext,
            ]),
        );
        let edit_mode = Box::new(Emacs::new(keybindings));
        let mut line_editor = Reedline::create()
            .with_history(history)
            .with_completer(Box::new(completer))
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_edit_mode(edit_mode);

        let prompt = SimplePrompt;
        let mut is_first_prompt = true;

        loop {
            if !is_first_prompt {
                self.renderer.write_output("\n");
            }
            is_first_prompt = false;

            let quota = engine.quota_display();
            self.renderer.print_session_status(&SessionStatus {
                model: self.config.model.clone(),
                provider: self.config.provider.clone(),
                context: engine.context_remaining_display(),
                quota: quota.clone(),
                auto_approve: self.config.auto_approve,
            });

            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buffer)) => {
                    let input = buffer.trim();
                    if input.is_empty() {
                        continue;
                    }
                    if input.starts_with('/') {
                        self.renderer.write_output("\n");
                    }
                    let mut cmd_ctx = crate::repl::commands::SlashCommandContext {
                        config: &mut self.config,
                        auth_store: &mut self.auth_store,
                        renderer: &self.renderer,
                        commands: Some(&self.commands),
                        session_id: Some(&engine.session_manager.session_id),
                        session_manager: Some(&engine.session_manager),
                    };
                    if let Some(cmd_res) = SlashCommandHandler::handle(input, &mut cmd_ctx).await? {
                        match cmd_res {
                            CommandResult::Exit => break,
                            CommandResult::ClearContext => {
                                engine =
                                    crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), None)
                                        .await?;
                                continue;
                            }
                            CommandResult::ModelChanged {
                                new_model,
                                new_provider,
                            } => {
                                self.config.model = new_model;
                                if let Some(provider) = new_provider {
                                    self.config.provider = provider;
                                }
                                engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                continue;
                            }
                            CommandResult::Login { provider } => {
                                crate::cli::login_provider(provider.as_deref(), &self.config, &mut self.auth_store)
                                    .await?;
                                engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                continue;
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
                                continue;
                            }
                            CommandResult::Tree => {
                                let tree = engine.session_manager.load_tree().await?;
                                let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
                                self.renderer.print_notice(&format!(
                                    "\nConversation Tree (Session: {}):\n{rendered}\n",
                                    engine.session_manager.session_id
                                ));
                                continue;
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
                                    && let Ok(true) = inquire::Confirm::new(
                                        "Summarize discoveries from abandoned branch before switching?",
                                    )
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
                                engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                self.renderer
                                    .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
                                continue;
                            }
                            CommandResult::ForkSession { turn_or_node_id } => {
                                let forked = engine
                                    .session_manager
                                    .fork_session(&self.config.sessions_dir, turn_or_node_id.as_deref())
                                    .await?;
                                self.renderer
                                    .print_notice(&format!("  [Forked session into {}]\n", forked.session_id));
                                continue;
                            }
                            CommandResult::CloneSession => {
                                let cloned = engine.session_manager.clone_session(&self.config.sessions_dir).await?;
                                self.renderer
                                    .print_notice(&format!("  [Cloned session into {}]\n", cloned.session_id));
                                continue;
                            }
                            CommandResult::ResumeSession { session_id } => {
                                engine = crate::platform::agent_engine(
                                    self.config.clone(),
                                    self.auth_store.clone(),
                                    Some(&session_id),
                                )
                                .await?;
                                self.renderer
                                    .print_notice(&format!("  [Resumed session {session_id}]\n"));
                                continue;
                            }
                            CommandResult::NameSession { name } => {
                                engine.session_manager.set_session_name(&name).await?;
                                self.renderer.print_notice(&format!("  [Named session: \"{name}\"]\n"));
                                continue;
                            }
                            CommandResult::ExpandedPrompt { text } => {
                                self.renderer.print_notice("  [Expanded template]\n");
                                self.renderer.print_user_block(&text);
                                self.renderer.write_output("\n");
                                self.run_agent_turn(&engine, crate::engine::runner::TurnRequest::new(&text))
                                    .await?;
                                engine.refresh_quota().await;
                                continue;
                            }
                            CommandResult::Rewind { turn } => {
                                let count = engine.session_manager.rewind_to_turn(turn).await?;
                                self.renderer.print_notice(&format!(
                                    "  [Rewound context to Turn {turn} ({count} messages retained)]\n"
                                ));
                                continue;
                            }
                            CommandResult::Logout { provider } => {
                                crate::cli::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                                continue;
                            }
                            CommandResult::Continue => continue,
                        }
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
                            continue;
                        }
                    }

                    let effective_input = if let Some(cmd) = input.strip_prefix('!') {
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

                    clear_submitted_input(input);
                    self.renderer.print_user_block(&effective_input);
                    self.renderer.write_output("\n");
                    self.run_agent_turn(&engine, crate::engine::runner::TurnRequest::new(&effective_input))
                        .await?;
                    engine.refresh_quota().await;
                }
                Ok(Signal::CtrlC) => {
                    self.renderer.write_output("\nCanceled input.\n");
                }
                Ok(Signal::CtrlD) => {
                    self.renderer.write_output("\nBye.\n");
                    break;
                }
                Err(err) => {
                    self.renderer.write_output(&format!("Input error: {err}\n"));
                    break;
                }
            }
        }

        Ok(())
    }

    async fn run_agent_turn(
        &self,
        engine: &AgentEngine,
        request: crate::engine::runner::TurnRequest<'_>,
    ) -> Result<()> {
        let renderer = &self.renderer;
        let run_future = engine.run_turn(request, std::sync::Arc::new(renderer.clone()));
        tokio::select! {
            run_res = run_future => {
                renderer.flush();
                renderer.write_output("\n");
                if let Err(error) = run_res {
                    renderer.write_output(&format!("\nError: {error}\n"));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                renderer.flush();
                engine.record_cancellation("operator interrupt").await?;
                renderer.write_output("\nCanceled.\n");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RhoCompleter, submitted_input_rows};
    use reedline::Completer;

    #[test]
    fn slash_commands_complete_from_a_prefix() {
        let mut completer = RhoCompleter::new(&[], Vec::new(), vec!["review".to_string()]);
        let suggestions = completer.complete("/mo", 3);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].value, "/model");

        let tmpl_suggestions = completer.complete("/rev", 4);
        assert_eq!(tmpl_suggestions.len(), 1);
        assert_eq!(tmpl_suggestions[0].value, "/review");
    }

    #[test]
    fn skill_names_complete_from_prefix() {
        let skill_names = crate::skills::resolved_skills(None, None)
            .into_iter()
            .map(|skill| skill.metadata.name)
            .collect();
        let mut completer = RhoCompleter::new(&[], skill_names, Vec::new());
        let suggestions = completer.complete("/skill pl", 9);
        assert!(suggestions.iter().any(|s| s.value == "/skill plan"));
    }

    #[test]
    fn submitted_input_rows_include_prompt_width_and_terminal_wrapping() {
        assert_eq!(submitted_input_rows("hello", 80), 1);
        assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
        assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
        assert_eq!(submitted_input_rows("界界", 5), 2);
    }
}
