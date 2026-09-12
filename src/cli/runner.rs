//! Single-turn execution runners for headless, JSON, and terminal output.

use crate::auth::AuthStore;
use crate::config::Config;
use std::sync::Arc;

pub struct CliRunner {
    pub config: Config,
    pub auth_store: AuthStore,
    pub resume_target: Option<String>,
}

fn handle_turn_result(
    res: rho_harness_core::error::Result<crate::engine::runner::TurnOutput>,
) -> Result<(), Box<dyn std::error::Error>> {
    match res {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e}");
            rho_engine::process::kill_all_tracked_processes();
            std::process::exit(1);
        }
    }
}

fn spawn_rpc_writer(
    mut event_rx: tokio::sync::mpsc::UnboundedReceiver<rho_harness_core::rpc::protocol::RpcEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut writer = rho_harness_core::rpc::transport::JsonLinesWriter::new(tokio::io::stdout());
        while let Some(event) = event_rx.recv().await {
            let _ = writer.write_message(&event).await;
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
        Arc::new(rho_harness_core::presentation::StructuredPresenter::stdout())
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

    pub async fn run_json_turn(self, prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
            Arc::new(crate::ui::render::RpcPresenter::new(event_tx));
        let writer_task = spawn_rpc_writer(event_rx);

        let engine = crate::platform::agent_engine(self.config, self.auth_store, self.resume_target.as_deref()).await?;
        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await;
        drop(presenter);
        let _ = writer_task.await;
        handle_turn_result(res)
    }

    pub async fn run_prompt_turn(
        self,
        prompt: &str,
        session_name: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let engine = crate::platform::agent_engine(self.config, self.auth_store, self.resume_target.as_deref()).await?;
        if let Some(name) = session_name {
            let _ = engine.session_manager.set_session_name(name).await;
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
