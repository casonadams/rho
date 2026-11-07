//! REPL frontends: `live` (raw-mode TUI with modals and streaming
//! transcript), `line_mode` (readline fallback), and the shared command,
//! completion, and coordination plumbing used by both.

pub mod commands;
pub mod completer;
pub mod coordinator;
pub(crate) mod input_reader;
pub mod interactive;
mod line_mode;
pub(crate) mod live;
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
        let mut renderer = TerminalRenderer::default();
        renderer.theme.apply_ui_config(&config.ui);
        Self {
            config,
            auth_store,
            renderer,
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
        config.sessions_dir = self.config.sessions_dir.clone();
        config.config_dir = self.config.config_dir.clone();
        config.auth_file = self.config.auth_file.clone();
        config.model = self.config.model.clone();
        config.provider = self.config.provider.clone();
        crate::repl::interactive::spawn_background_model_refresh(&config, &self.auth_store);
        let rebuilt = engine.rebuild(config.clone(), self.auth_store.clone()).await?;
        self.config = config;
        self.renderer.theme.apply_ui_config(&self.config.ui);

        let skills: Vec<String> = crate::skills::resolved_skills_async(std::env::current_dir().ok().as_deref())
            .await
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

    pub(crate) async fn sync_engine_model(&self, engine: &mut AgentEngine) {
        if self.config.model != engine.config.model
            || self.config.provider != engine.config.provider
            || self.config.thinking_level != engine.config.thinking_level
        {
            engine.config.model = self.config.model.clone();
            engine.config.provider = self.config.provider.clone();
            engine.config.thinking_level = self.config.thinking_level.clone();
            let _ = engine.update_model().await;
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let stdin_is_tty = std::io::stdin().is_tty();
        let stdout_is_tty = std::io::stdout().is_tty();
        if live::live_ui_supported(stdin_is_tty, stdout_is_tty) {
            self.run_live().await
        } else {
            line_mode::run_line_mode(self, stdin_is_tty).await
        }
    }
}

pub(crate) async fn init_engine(session: &mut ReplSession) -> Result<AgentEngine> {
    let engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;
    if let Some(name) = session.cli.as_ref().and_then(|cli| cli.name.as_deref()) {
        let _ = engine.session_manager.set_session_name(name).await;
    }
    session.config = engine.config.clone();
    engine.spawn_refresh_quota();
    Ok(engine)
}

pub(crate) async fn print_welcome(session: &ReplSession, engine: &AgentEngine) -> Vec<crate::skills::ResolvedSkill> {
    let skills = crate::skills::resolved_skills_async(std::env::current_dir().ok().as_deref()).await;
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let tools = engine.tool_names();
    let mcp = session.config.mcp.servers.keys().cloned().collect::<Vec<_>>();
    let agents = engine.instruction_files().await;
    session.renderer.print_welcome(&crate::ui::render::WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        mcp,
    });
    skills
}
