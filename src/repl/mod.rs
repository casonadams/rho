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
use rho_harness_core::session::SessionManager;

pub struct ReplSession {
    pub config: Config,
    pub auth_store: AuthStore,
    pub renderer: TerminalRenderer,
    pub resume_id: Option<String>,
    pub cli: Option<crate::config::cli::Cli>,
    pub(crate) mcp_completions: crate::repl::interactive::McpServerList,
    pub catalog: rho_engine::provider::ModelCatalog,
    fetch_models: bool,
}

impl ReplSession {
    pub fn new(config: Config, auth_store: AuthStore, resume_id: Option<String>) -> Self {
        let mut renderer = TerminalRenderer::default();
        renderer.theme.apply_ui_config(&config.ui);
        let mcp_completions = crate::repl::interactive::McpServerList::default();
        mcp_completions.set_from(&config.mcp);
        Self {
            config,
            auth_store,
            renderer,
            resume_id,
            cli: None,
            mcp_completions,
            catalog: rho_engine::provider::ModelCatalog::default(),
            fetch_models: false,
        }
    }

    /// Fetches live model lists into `catalog`; without this the session only shows what is put
    /// in `catalog` directly.
    pub fn with_live_models(mut self) -> Self {
        self.fetch_models = true;
        self
    }

    /// A fresh engine for this session's config, sharing its model catalog.
    pub(crate) async fn new_engine(&self, resume: Option<&str>) -> Result<AgentEngine> {
        crate::platform::agent_engine(
            self.config.clone(),
            self.auth_store.clone(),
            self.catalog.clone(),
            resume,
        )
        .await
    }

    /// Re-fetches every reachable provider's model list in the background.
    pub(crate) fn refresh_model_catalog(&self) {
        if self.fetch_models {
            self.catalog.spawn_refresh_all(&self.config, &self.auth_store);
        }
    }

    pub(crate) fn model_list(&self) -> crate::repl::interactive::ModelList {
        crate::repl::interactive::ModelList::live(
            self.config.clone(),
            self.catalog.clone(),
            self.auth_store.stored_logins(),
        )
    }

    /// Every model the session can switch to right now, across all logins.
    pub(crate) fn listed_models(&self) -> Vec<crate::repl::interactive::ModelItem> {
        crate::repl::interactive::discover_models(&self.config, &self.catalog, &self.auth_store.stored_logins())
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
        for warning in &config.config_warnings {
            self.renderer.notice(crate::ui::render::NoticeKind::Warn, warning);
        }
        config.sessions_dir = self.config.sessions_dir.clone();
        config.config_dir = self.config.config_dir.clone();
        config.auth_file = self.config.auth_file.clone();
        config.model = self.config.model.clone();
        config.provider = self.config.provider.clone();
        let rebuilt = engine.rebuild(config.clone(), self.auth_store.clone()).await?;
        self.config = config;
        self.refresh_model_catalog();
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
        report_mcp_diagnostics(self, &rebuilt);
        Ok(rebuilt)
    }

    pub(crate) fn sync_engine_settings(&self, engine: &mut AgentEngine) {
        engine.config.models = self.config.models.clone();
        engine.config.tools.web = self.config.tools.web.clone();
        engine.config.mcp.enabled = self.config.mcp.enabled;
        engine.config.permission.enabled = self.config.permission.enabled;
        engine.config.ui = self.config.ui.clone();
        engine.config.show_label = self.config.show_label;
    }

    pub(crate) async fn sync_engine_model(&self, engine: &mut AgentEngine) {
        self.sync_engine_settings(engine);
        if self.config.model != engine.config.model
            || self.config.provider != engine.config.provider
            || self.config.thinking_level != engine.config.thinking_level
        {
            engine.config.model = self.config.model.clone();
            engine.config.provider = self.config.provider.clone();
            engine.config.thinking_level = self.config.thinking_level.clone();
            let result = engine.update_model().await;
            live::navigation::report_model_update(self, engine, result, "Could not update model");
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
    let engine = session.new_engine(session.resume_id.as_deref()).await?;
    if let Some(name) = session.cli.as_ref().and_then(|cli| cli.name.as_deref()) {
        let _ = engine.session_manager.set_session_name(name).await;
    }
    session.config = engine.config.clone();
    engine.spawn_refresh_quota();
    Ok(engine)
}

/// Command that resumes the session, or `None` when it has no content worth resuming.
pub(crate) async fn resume_hint(session_manager: &SessionManager) -> Option<String> {
    let tree = session_manager.load_tree().await.ok()?;
    (!tree.is_empty() || tree.session_name.is_some())
        .then(|| format!("Resume this session:\nrho --resume {}", session_manager.session_id))
}

pub(crate) async fn print_resume_hint(engine: &AgentEngine) {
    if let Some(hint) = resume_hint(&engine.session_manager).await {
        eprintln!("{hint}");
    }
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
    report_mcp_diagnostics(session, engine);
    skills
}

pub(crate) fn report_mcp_diagnostics(session: &ReplSession, engine: &AgentEngine) {
    session.mcp_completions.set_from(&engine.config.mcp);
    warn_mcp(session, engine.mcp_diagnostics());
}

pub(crate) fn report_late_mcp_problems(session: &ReplSession, engine: &AgentEngine) {
    if let Some(runtime) = engine.mcp_runtime() {
        warn_mcp(session, &runtime.take_problems());
    }
}

fn warn_mcp(session: &ReplSession, diagnostics: &[rho_engine::mcp::McpDiagnostic]) {
    let lines = crate::cli::mcp::mcp_diagnostic_lines(diagnostics);
    if !lines.is_empty() {
        session
            .renderer
            .notice(crate::ui::render::NoticeKind::Warn, &lines.join("\n"));
    }
}
