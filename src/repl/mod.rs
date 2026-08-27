pub mod commands;
mod prompt;

pub use prompt::SimplePrompt;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use crate::ui::TerminalRenderer;
use reedline::{FileBackedHistory, Reedline, Signal};

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
        self.renderer.print_welcome(&self.config.model, &self.config.provider);

        let history_file = self.config.config_dir.join("history.txt");
        let history =
            Box::new(FileBackedHistory::with_file(1000, history_file).unwrap_or_else(|_| FileBackedHistory::default()));

        let mut line_editor = Reedline::create().with_history(history);

        let mut engine =
            AgentEngine::new(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref()).await?;

        let prompt = SimplePrompt;
        let mut is_first_prompt = true;

        loop {
            if !is_first_prompt {
                println!();
            }
            is_first_prompt = false;

            let dim = self.renderer.theme.dimmed;
            println!("{dim}{}:{}{dim:#}", self.config.model, engine.context_usage_display());

            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buffer)) => {
                    let input = buffer.trim();
                    if input.is_empty() {
                        continue;
                    }
                    println!();

                    if let Some(cmd_res) = SlashCommandHandler::handle(input, &mut self.config, &mut self.auth_store)? {
                        match cmd_res {
                            CommandResult::Exit => break,
                            CommandResult::ClearContext => {
                                engine = AgentEngine::new(self.config.clone(), self.auth_store.clone(), None).await?;
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

                    self.run_agent_turn(&engine, crate::engine::runner::TurnRequest { prompt: input })
                        .await?;
                }
                Ok(Signal::CtrlC) => {
                    println!("\n  [Operation canceled]");
                }
                Ok(Signal::CtrlD) => {
                    println!("\n  Bye!");
                    break;
                }
                Err(err) => {
                    eprintln!("REPL error: {err}");
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
        let run_future = engine.run_turn(request, renderer);
        tokio::select! {
            run_res = run_future => {
                renderer.flush();
                println!();
                if let Err(error) = run_res {
                    eprintln!("\n  Error: {error}");
                }
            }
            _ = tokio::signal::ctrl_c() => {
                renderer.flush();
                engine.record_cancellation("operator interrupt").await?;
                println!("\n  [Operation canceled]");
            }
        }
        Ok(())
    }
}
