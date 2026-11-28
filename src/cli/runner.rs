//! Single-turn execution runners for headless, JSON, and terminal output.

use super::CliResult;
use crate::auth::AuthStore;
use crate::config::Config;
use std::sync::Arc;

pub struct CliRunner {
    pub config: Config,
    pub auth_store: AuthStore,
    pub resume_target: Option<String>,
}

fn handle_turn_result(res: rho_harness_core::error::Result<crate::engine::runner::TurnOutput>) -> CliResult {
    match res {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e}");
            rho_engine::process::shutdown_children();
            std::process::exit(1);
        }
    }
}

fn spawn_rpc_writer(events: crate::cli::rpc::RpcEventQueue) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut writer = rho_harness_core::rpc::transport::JsonLinesWriter::new(tokio::io::stdout());
        while let Some(batch) = events.next_batch().await {
            for event in batch {
                if let Err(err) = writer.write_message(&event).await {
                    eprintln!("Error: failed to write RPC event: {err}");
                    return;
                }
            }
        }
    })
}

fn default_terminal_presenter() -> Arc<dyn rho_harness_core::presentation::Presenter> {
    #[cfg(feature = "ui")]
    {
        let renderer = crate::ui::TerminalRenderer::default();
        if std::io::IsTerminal::is_terminal(&std::io::stdout())
            && let Ok((cols, _)) = crossterm::terminal::size()
        {
            renderer.set_width(cols as usize);
        }
        Arc::new(renderer)
    }
    #[cfg(not(feature = "ui"))]
    {
        Arc::new(super::ndjson::stdout_presenter())
    }
}

impl CliRunner {
    pub fn new(config: Config, auth_store: AuthStore, resume_target: Option<String>) -> Self {
        Self {
            config,
            auth_store,
            resume_target,
        }
    }

    async fn build_engine(self) -> CliResult<crate::engine::AgentEngine> {
        let catalog = rho_engine::provider::ModelCatalog::default();
        catalog.spawn_refresh(&self.config.provider, &self.config, &self.auth_store);
        Ok(crate::platform::agent_engine(self.config, self.auth_store, catalog, self.resume_target.as_deref()).await?)
    }

    pub async fn run_json_turn(self, prompt: &str) -> CliResult {
        let events = crate::cli::rpc::RpcEventQueue::default();
        let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
            Arc::new(crate::cli::rpc::RpcPresenter::new(events.clone()));
        let writer_task = spawn_rpc_writer(events.clone());

        let engine = self.build_engine().await?;
        crate::cli::mcp::eprint_mcp_diagnostics(&engine);
        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await;
        drop(presenter);
        events.close();
        let _ = writer_task.await;
        handle_turn_result(res)
    }

    pub async fn run_prompt_turn(self, prompt: &str, session_name: Option<&str>) -> CliResult {
        let engine = self.build_engine().await?;
        crate::cli::mcp::eprint_mcp_diagnostics(&engine);
        if let Some(name) = session_name {
            engine.session_manager.set_session_name(name).await?;
        }
        let presenter = default_terminal_presenter();
        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await;
        presenter.flush();

        #[cfg(feature = "ui")]
        println!();

        handle_turn_result(res)
    }
}
