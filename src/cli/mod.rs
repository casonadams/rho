pub mod auth;
mod commands;
pub mod rpc;
mod runner;
mod session;

#[cfg(test)]
mod tests;

pub use auth::{login_provider, logout_provider};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::Cli;
#[cfg(feature = "ui")]
use crate::repl::ReplSession;
use std::io::Read;

pub async fn run_cli() -> std::result::Result<(), Box<dyn std::error::Error>> {
    struct ProcessCleanupGuard;
    impl Drop for ProcessCleanupGuard {
        fn drop(&mut self) {
            rho_engine::process::kill_all_tracked_processes();
        }
    }
    let _process_cleanup = ProcessCleanupGuard;

    let cli = <Cli as clap::Parser>::parse();
    let config = Config::load(Some(&cli))?;
    let cli_for_repl = cli.clone();
    config.ensure_dirs()?;

    let mut auth_store = AuthStore::load(&config.auth_file)?;

    if let Some(cmd) = cli.command {
        return commands::handle_command(cmd, &config, &mut auth_store).await;
    }

    let prompt_text = resolve_prompt_text(&cli);
    let resume_target = session::resolve_resume_target(&cli, &config)?;

    if let Some(export_path) = cli.export {
        return session::export_session(&export_path, resume_target, &config).await;
    }

    if cli.mode == "rpc" {
        return rpc::run_rpc_daemon(config, auth_store).await.map_err(Into::into);
    }

    if cli.mode == "json"
        && let Some(prompt) = prompt_text.as_deref()
    {
        let runner = runner::CliRunner::new(config, auth_store, resume_target);
        return runner.run_json_turn(prompt).await;
    }

    if let Some(prompt) = prompt_text.as_deref() {
        let runner = runner::CliRunner::new(config, auth_store, resume_target);
        return runner.run_prompt_turn(prompt, cli.name.as_deref()).await;
    }

    #[cfg(feature = "ui")]
    {
        let mut session = ReplSession::new(config, auth_store, resume_target).with_cli(Some(cli_for_repl));
        session.run().await?;
        Ok(())
    }
    #[cfg(not(feature = "ui"))]
    {
        let _ = cli_for_repl;
        Err(Box::new(crate::error::AppError::Session(
            "interactive REPL is unavailable in headless mode (compiled without 'ui' feature); provide a prompt via -p or piped stdin".to_string(),
        )))
    }
}

fn resolve_prompt_text(cli: &Cli) -> Option<String> {
    if let Some(p) = cli.prompt.clone() {
        Some(p)
    } else if !cli.message.is_empty() {
        Some(cli.message.join(" "))
    } else if !atty_check() {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer).ok();
        let trimmed = buffer.trim().to_string();
        if !trimmed.is_empty() { Some(trimmed) } else { None }
    } else {
        None
    }
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
