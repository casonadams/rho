pub mod commands;
pub mod completer;
pub mod coordinator;
mod input_reader;
pub mod interactive;
mod line_mode;
mod live;
mod prompt;
#[cfg(test)]
mod tests;

pub use completer::RhoCompleter;
#[cfg(test)]
pub(crate) use line_mode::submitted_input_rows;
pub use prompt::SimplePrompt;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crossterm::tty::IsTty;

pub struct ReplSession {
    pub config: Config,
    pub auth_store: AuthStore,
    pub renderer: TerminalRenderer,
    pub resume_id: Option<String>,
    pub cli: Option<crate::config::cli::Cli>,
}

impl ReplSession {
    pub fn new(config: Config, auth_store: AuthStore, resume_id: Option<String>) -> Self {
        Self {
            config,
            auth_store,
            renderer: TerminalRenderer::default(),
            resume_id,
            cli: None,
        }
    }

    /// Retained so /reload can re-apply CLI overrides after re-reading config.
    pub fn with_cli(mut self, cli: Option<crate::config::cli::Cli>) -> Self {
        self.cli = cli;
        self
    }

    /// Re-read config (keeping CLI overrides and the runtime model choice),
    /// rebuild the engine, and preserve the session history.
    pub(crate) async fn reload_engine(&mut self, engine: &AgentEngine) -> Result<AgentEngine> {
        let mut config = Config::load(self.cli.as_ref())?;
        config.model = self.config.model.clone();
        config.provider = self.config.provider.clone();
        crate::repl::interactive::spawn_background_model_refresh(&config, &self.auth_store);
        let rebuilt = engine.rebuild(config.clone(), self.auth_store.clone()).await?;
        self.config = config;

        let skills: Vec<String> = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref())
            .into_iter()
            .map(|s| s.metadata.name)
            .collect();
        let tools = rebuilt.tool_names();

        self.renderer.print_notice(&format!(
            "  [Reloaded config, skills, and tools ({} skills, {} tools); session preserved]\n",
            skills.len(),
            tools.len()
        ));
        Ok(rebuilt)
    }

    pub async fn run(&mut self) -> Result<()> {
        let stdin_is_tty = std::io::stdin().is_tty();
        let stdout_is_tty = std::io::stdout().is_tty();
        if live::live_ui_supported(stdin_is_tty, stdout_is_tty) {
            return self.run_live().await;
        }

        line_mode::run_line_mode(self, stdin_is_tty).await
    }
}
