//! CLI subcommand routing: `run`/`auth`/`mcp`/`update`/`rpc` dispatch, credential
//! flows, self-update, and process cleanup guards.

pub mod auth;
mod commands;
mod exit;
pub mod mcp;
pub mod ndjson;
pub mod rpc;
mod runner;
pub mod self_update;
mod session;
pub mod stats;
pub mod worktree;

#[cfg(test)]
mod tests;

pub use auth::{login_provider, logout_provider};

pub(crate) type CliResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::Cli;
#[cfg(feature = "ui")]
use crate::repl::ReplSession;
use std::io::Read;

struct ProcessCleanupGuard;
impl Drop for ProcessCleanupGuard {
    fn drop(&mut self) {
        rho_engine::process::shutdown_children();
    }
}

#[cfg(feature = "ui")]
async fn run_interactive_session(
    config: Config,
    auth_store: AuthStore,
    resume_target: Option<String>,
    cli: Cli,
) -> CliResult {
    let mut session = ReplSession::new(config, auth_store, resume_target)
        .with_cli(Some(cli))
        .with_live_models();
    session.run().await?;
    Ok(())
}

#[cfg(not(feature = "ui"))]
async fn run_interactive_session(
    _config: Config,
    _auth_store: AuthStore,
    _resume_target: Option<String>,
    _cli: Cli,
) -> CliResult {
    Err(Box::new(crate::error::AppError::Session(
        "interactive REPL is unavailable in headless mode (compiled without 'ui' feature); provide a prompt via -p or piped stdin".to_string(),
    )))
}

async fn dispatch_prompt_runner(runner: runner::CliRunner, prompt: &str, mode: &str, name: Option<&str>) -> CliResult {
    if mode == "json" {
        runner.run_json_turn(prompt).await
    } else {
        runner.run_prompt_turn(prompt, name).await
    }
}

async fn dispatch_cli_run(cli: Cli, config: Config, auth_store: AuthStore) -> CliResult {
    let resume_target = session::resolve_resume_target(&cli, &config)?;
    if let Some(export_path) = cli.export.as_deref() {
        return session::export_session(export_path, resume_target, &config).await;
    }
    if let Some(ref session_id) = resume_target {
        session::restore_session_cwd(&config.sessions_dir, session_id);
    }
    if cli.mode == "rpc" {
        return rpc::run_rpc_daemon(config, auth_store).await.map_err(Into::into);
    }
    if let Some(prompt) = resolve_prompt_text(&cli)? {
        let runner = runner::CliRunner::new(config, auth_store, resume_target);
        return dispatch_prompt_runner(runner, &prompt, &cli.mode, cli.name.as_deref()).await;
    }
    run_interactive_session(config, auth_store, resume_target, cli).await
}

pub async fn run_cli() -> CliResult {
    rho_engine::install_crypto_provider();
    let _process_cleanup = ProcessCleanupGuard;
    exit::install_exit_handlers()?;

    let cli = <Cli as clap::Parser>::parse();
    let config = Config::load(Some(&cli))?;
    for warning in &config.config_warnings {
        eprintln!("Warning: {warning}");
    }
    config.ensure_dirs_async().await?;

    let mut auth_store = AuthStore::load_async(&config.auth_file).await?;
    if let Some(cmd) = cli.command {
        return commands::handle_command(cmd, &config, &mut auth_store).await;
    }

    let result = dispatch_cli_run(cli, config, auth_store).await;
    rho_engine::worktree::settle_abandoned().await;
    result
}

fn resolve_prompt_text(cli: &Cli) -> CliResult<Option<String>> {
    if let Some(p) = cli.prompt.clone() {
        Ok(Some(p))
    } else if !cli.message.is_empty() {
        Ok(Some(cli.message.join(" ")))
    } else if !atty_check() {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        let trimmed = buffer.trim().to_string();
        Ok(if trimmed.is_empty() { None } else { Some(trimmed) })
    } else {
        Ok(None)
    }
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
