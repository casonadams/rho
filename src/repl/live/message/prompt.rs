use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::live::turn::run_active_turn;
use crate::repl::live::{ActiveTurn, LiveIo, LiveMessage};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, TerminalBackend};

async fn run_discarding_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let _ = crate::repl::live::bash_runner::run_user_bash(cmd, renderer, io).await;
    Ok(None)
}

async fn run_prompt_with_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let res = crate::repl::live::bash_runner::run_user_bash(cmd, renderer, io).await?;
    if res.is_cancelled {
        return Ok(None);
    }
    let status = if res.is_error { " (failed)" } else { "" };
    Ok(Some(format!(
        "Executed local shell command: `{cmd}`{status}\n\nOutput:\n```\n{}\n```",
        res.output
    )))
}

pub(crate) async fn resolve_effective_prompt<B: TerminalBackend>(
    input: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    if let Some(cmd) = input.strip_prefix("!!").map(str::trim).filter(|c| !c.is_empty()) {
        return run_discarding_output(cmd, renderer, io).await;
    }
    if let Some(cmd) = input.strip_prefix('!').map(str::trim).filter(|c| !c.is_empty()) {
        return run_prompt_with_output(cmd, renderer, io).await;
    }
    Ok(Some(input.to_string()))
}

pub(crate) fn is_slash_input(input: &str) -> bool {
    crate::repl::commands::is_slash_command(input)
}

pub(crate) async fn run_slash_handler(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    input: &str,
) -> Result<Option<CommandResult>> {
    let mut command_context = SlashCommandContext {
        config: &mut session.config,
        auth_store: &mut session.auth_store,
        renderer: &session.renderer,
        session_id: Some(&engine.session_manager.session_id),
        session_manager: Some(&engine.session_manager),
        engine: Some(engine),
        home_dir: None,
    };
    SlashCommandHandler::handle(input, &mut command_context).await
}

pub(crate) async fn run_prompt_turn<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    live: LiveMessage<'_, B>,
    effective: String,
) -> Result<bool> {
    live.io.controller.state_mut().footer_mut().activity = Activity::Working;
    session.renderer.print_user_block(&effective);
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TurnStart {
        turn_number: 1,
        prompt: effective.clone(),
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::StatusChanged {
        status: "busy".to_string(),
    });
    run_active_turn(
        session,
        engine,
        ActiveTurn {
            io: live.io,
            editor: live.editor,
            prompt: &effective,
        },
    )
    .await?;
    session.sync_engine_model(engine).await;
    engine.refresh_quota().await;
    let totals = engine.session_usage_totals();
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: engine.context_percent_f64(),
        context_window: engine.context_limit(),
        tokens_per_second: engine.tokens_per_second(),
        quota: engine.quota_display(),
    });
    Ok(false)
}
