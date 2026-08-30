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
use std::io::Write;
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
    pub fn new(extension_commands: &[(&str, &str)], skill_names: Vec<String>) -> Self {
        Self {
            completions: CompletionSet::rho(extension_commands, skill_names),
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
}

impl ReplSession {
    pub fn new(config: Config, auth_store: AuthStore, resume_id: Option<String>) -> Self {
        Self {
            config,
            auth_store,
            renderer: TerminalRenderer::default(),
            resume_id,
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
            model: &self.config.model,
            provider: &self.config.provider,
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
        });

        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        engine.refresh_quota().await;

        let history_file = self.config.config_dir.join("history.txt");
        let history =
            Box::new(FileBackedHistory::with_file(1000, history_file).unwrap_or_else(|_| FileBackedHistory::default()));
        let skill_names =
            crate::skills::resolved_skills(Some(&self.config.config_dir), Some(&std::env::current_dir()?))
                .into_iter()
                .map(|skill| skill.metadata.name)
                .collect();
        let completer = RhoCompleter::new(&engine.extension_registry.list_commands(), skill_names);
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
                model: &self.config.model,
                provider: &self.config.provider,
                context: &engine.context_remaining_display(),
                quota: quota.as_deref(),
                auto_approve: self.config.auto_approve,
            });

            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buffer)) => {
                    let input = buffer.trim();
                    if input.is_empty() {
                        continue;
                    }
                    let ext_ctx = engine.extension_context();
                    if input.starts_with('/') {
                        self.renderer.write_output("\n");
                    }
                    let mut cmd_ctx = crate::repl::commands::SlashCommandContext {
                        config: &mut self.config,
                        auth_store: &mut self.auth_store,
                        registry: Some(&engine.extension_registry),
                        context: Some(&ext_ctx),
                        renderer: &self.renderer,
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
                            CommandResult::Logout { provider } => {
                                crate::cli::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                                continue;
                            }
                            CommandResult::Continue => continue,
                        }
                    }

                    let effective_input = match engine.extension_registry.dispatch_input(input, &ext_ctx).await? {
                        crate::plugin::InputAction::Continue => input.to_string(),
                        crate::plugin::InputAction::Transform(transformed) => transformed,
                        crate::plugin::InputAction::Handled { output } => {
                            if !output.is_empty() {
                                self.renderer.write_output(&format!("{output}\n"));
                            }
                            continue;
                        }
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
        let mut completer = RhoCompleter::new(&[], Vec::new());
        let suggestions = completer.complete("/mo", 3);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].value, "/model");
    }

    #[test]
    fn skill_names_complete_from_prefix() {
        let skill_names = crate::skills::resolved_skills(None, None)
            .into_iter()
            .map(|skill| skill.metadata.name)
            .collect();
        let mut completer = RhoCompleter::new(&[], skill_names);
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
