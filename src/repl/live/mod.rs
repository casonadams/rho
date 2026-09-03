pub mod autocomplete;
pub mod batch;
pub mod idle;
pub mod message;
pub mod modal;
pub mod navigation;
#[cfg(test)]
mod tests;
pub mod turn;

pub use batch::LiveController;

use batch::drain_ui_events;
use idle::read_idle_input;
use navigation::update_footer;
use tokio::sync::mpsc;

use super::ReplSession;
use super::input_reader::TerminalInputReader;
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveState, QueuedMessage, TerminalController, UiEvent};
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

pub struct IdleContext<'a, 'b> {
    pub io: LiveIo<'a>,
    pub editor: EditorResources<'a>,
    pub session: &'b mut ReplSession,
    pub engine: &'b mut crate::engine::AgentEngine,
}

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

impl ReplSession {
    pub(super) async fn run_live(&mut self) -> Result<()> {
        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        if let Some(ref cli) = self.cli
            && let Some(ref name) = cli.name
        {
            let _ = engine.session_manager.set_session_name(name).await;
        }
        self.config = engine.config.clone();
        engine.refresh_quota().await;

        let (ui, mut ui_events) = crate::ui::interactive::InteractiveUi::channel();
        self.renderer = TerminalRenderer::with_ui(ui);
        let mut state = InteractiveState::default();
        update_footer(&mut state, self, &engine);
        let mut controller = TerminalController::stdout(state)?;
        let mut input = TerminalInputReader::spawn()?;
        let skills =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref());
        let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
        let tools = engine.tool_names.clone();
        let mut plugins = self.config.plugins.keys().cloned().collect::<Vec<_>>();
        for mcp in self.config.mcp.servers.keys() {
            if !plugins.contains(mcp) {
                plugins.push(mcp.clone());
            }
        }

        self.renderer.print_welcome(&WelcomeDisplay {
            model: self.config.model.clone(),
            provider: self.config.provider.clone(),
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
            tools,
            skills: skill_names,
            plugins,
        });
        drain_ui_events(&mut controller, &mut ui_events, &mut None)?;

        let mut history = InteractiveHistory::with_file(1000, self.config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?;
        let prompt_templates = rho_harness_core::prompts::discover_prompt_templates(
            Some(&self.config.config_dir),
            std::env::current_dir().ok().as_deref(),
        )
        .into_iter()
        .map(|t| t.metadata.name)
        .collect::<Vec<_>>();
        crate::repl::interactive::spawn_background_model_refresh(&self.config, &self.auth_store);
        let models = crate::repl::interactive::discover_models(&self.config, &self.auth_store);
        let custom_providers = self.config.providers.keys().cloned().collect();
        let sources = crate::repl::interactive::CompletionSources::new()
            .with_skills(skills)
            .with_templates(prompt_templates)
            .with_models(models)
            .with_custom_providers(custom_providers);
        let completions = CompletionSet::from_sources(sources);

        if self.resume_id.is_some()
            && let Ok(tree) = engine.session_manager.load_tree().await
        {
            let _ = navigation::hydrate_session_transcript(&mut controller, &tree, &mut history);
        }

        loop {
            let message = match controller.state_mut().pop_queued() {
                Some(message) => message,
                None => match read_idle_input(IdleContext {
                    io: LiveIo {
                        controller: &mut controller,
                        events: &mut ui_events,
                        input: &mut input,
                    },
                    editor: EditorResources {
                        history: &mut history,
                        completions: &completions,
                    },
                    session: self,
                    engine: &mut engine,
                })
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
        Ok(())
    }
}
