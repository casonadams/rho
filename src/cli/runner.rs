//! Single-turn execution runners for headless, JSON, and terminal output.

use crate::auth::AuthStore;
use crate::config::Config;
use std::sync::Arc;

pub struct CliRunner {
    pub config: Config,
    pub auth_store: AuthStore,
    pub resume_target: Option<String>,
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
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<rho_harness_core::rpc::protocol::RpcEvent>();
        let (presenter, _) = crate::ui::render::RpcPresenter::new(event_tx);
        let presenter_arc: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(presenter);

        let writer_task = tokio::spawn(async move {
            let mut writer = rho_harness_core::rpc::transport::JsonLinesWriter::new(tokio::io::stdout());
            while let Some(event) = event_rx.recv().await {
                let _ = writer.write_message(&event).await;
            }
        });

        let engine = crate::platform::agent_engine(self.config, self.auth_store, self.resume_target.as_deref()).await?;
        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter_arc.clone())
            .await;
        drop(presenter_arc);
        let _ = writer_task.await;

        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Error: {e}");
                rho_engine::process::kill_all_tracked_processes();
                std::process::exit(1);
            }
        }
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
        #[cfg(feature = "ui")]
        let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
            Arc::new(crate::ui::TerminalRenderer::default());
        #[cfg(not(feature = "ui"))]
        let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
            Arc::new(rho_harness_core::presentation::StructuredPresenter::stdout());

        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await;
        presenter.flush();

        #[cfg(feature = "ui")]
        println!();

        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Error: {e}");
                rho_engine::process::kill_all_tracked_processes();
                std::process::exit(1);
            }
        }
    }
}
