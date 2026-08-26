pub mod commands;
mod recovery;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::intent::clarify::ClarificationHandler;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use crate::ui::TerminalRenderer;
use reedline::{FileBackedHistory, Prompt, PromptEditMode, PromptHistorySearch, Reedline, Signal};
use std::borrow::Cow;

pub struct SimplePrompt;

impl Prompt for SimplePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed(". ")
    }

    fn render_prompt_history_search_indicator(&self, _history_search: PromptHistorySearch) -> Cow<'_, str> {
        Cow::Borrowed("search: ")
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
        self.renderer.print_welcome(&self.config.model, &self.config.provider);

        let history_file = self.config.config_dir.join("history.txt");
        let history =
            Box::new(FileBackedHistory::with_file(1000, history_file).unwrap_or_else(|_| FileBackedHistory::default()));

        let mut line_editor = Reedline::create().with_history(history);

        let recovered = if self.resume_id.is_none() {
            recovery::recover_session(&self.config, &self.auth_store)?
        } else {
            None
        };
        if let Some(recovered) = &recovered {
            self.resume_id = Some(recovered.session_id.clone());
        }
        let mut engine =
            AgentEngine::new(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref()).await?;
        if let Some(recovered) = &recovered {
            println!("Continuing: {}\n", recovered.spec.objective);
            self.run_agent_turn(
                &engine,
                crate::engine::runner::TurnRequest {
                    prompt: "Continue the active IntentSpec from its remaining outcomes and verification obligations.",
                    intent: Some(&recovered.spec),
                },
            )
            .await?;
        }

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

                    // Check slash command
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
                                crate::login_provider(provider.as_deref(), &self.config, &mut self.auth_store).await?;
                                engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                continue;
                            }
                            CommandResult::Logout { provider } => {
                                crate::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                                continue;
                            }
                            CommandResult::Continue => continue,
                        }
                    }

                    let mut turn_prompt = input.to_string();
                    let mut continued_spec = None;
                    if let Some(state) = engine.current_intent_state()?
                        && state.is_unfinished()
                    {
                        self.renderer.print_unfinished_intent(&state);
                        let choice = inquire::Select::new(
                            "Handle this input:",
                            vec![
                                "Continue current task",
                                "Amend current focus",
                                "Mark current task complete",
                                "Pause and switch tasks",
                            ],
                        )
                        .prompt();
                        println!();
                        match choice {
                            Ok("Continue current task") => {
                                turn_prompt = "Continue the active IntentSpec from its remaining outcomes and verification obligations.".to_string();
                                continued_spec = Some(state.spec);
                            }
                            Ok("Mark current task complete") => {
                                engine.complete_current_intent_by_user()?;
                                self.resume_id = None;
                                engine = AgentEngine::new(self.config.clone(), self.auth_store.clone(), None).await?;
                                println!("Intent marked complete. Enter a new task.");
                                continue;
                            }
                            Ok("Pause and switch tasks") => {
                                engine.pause_current_intent()?;
                                self.resume_id = None;
                                engine = AgentEngine::new(self.config.clone(), self.auth_store.clone(), None).await?;
                            }
                            Ok("Amend current focus") => {}
                            Ok(_) | Err(_) => continue,
                        }
                    }

                    let intent_spec = match continued_spec {
                        Some(spec) => spec,
                        None => ClarificationHandler::process_intent(&turn_prompt, true)?,
                    };

                    self.run_agent_turn(
                        &engine,
                        crate::engine::runner::TurnRequest {
                            prompt: &turn_prompt,
                            intent: Some(&intent_spec),
                        },
                    )
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
                match run_res {
                    Ok(_) => {
                        if let Some(state) = engine.current_intent_state()? {
                            renderer.print_unfinished_intent(&state);
                        }
                    }
                    Err(error) => eprintln!("\n  Error: {error}"),
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
