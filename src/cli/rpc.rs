use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::ui::render::RpcPresenter;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use std::sync::Arc;
use tokio::io::BufReader;
use tokio::sync::mpsc;

enum RpcLoopAction {
    Continue,
    Break,
}

enum NextRpcItem {
    Event(Option<RpcEvent>),
    Request(rho_harness_core::error::Result<Option<RpcRequest>>),
}

struct RpcDaemonContext<'a, W> {
    writer: &'a mut JsonLinesWriter<W>,
    engine: &'a mut rho_engine::engine::AgentEngine,
    presenter: &'a Arc<dyn rho_harness_core::presentation::Presenter>,
    config: &'a Config,
    auth_store: &'a AuthStore,
}

async fn handle_compact_cmd(
    engine: &rho_engine::engine::AgentEngine,
    instructions: Option<String>,
    req_id: Option<String>,
) -> RpcResponse {
    match engine.compact_session(instructions.as_deref()).await {
        Ok(s) => RpcResponse::success(
            req_id,
            "compact",
            Some(serde_json::json!({
                "tokens_before": s.tokens_before,
                "tokens_after": s.tokens_after,
                "saved_tokens": s.saved_tokens,
            })),
        ),
        Err(e) => RpcResponse::failure(req_id, "compact", &e.to_string()),
    }
}

async fn handle_set_model_cmd(
    engine: &mut rho_engine::engine::AgentEngine,
    (config, auth_store): (&Config, &AuthStore),
    (model, provider, req_id): (String, Option<String>, Option<String>),
) -> (RpcResponse, Option<rho_engine::engine::AgentEngine>) {
    let mut new_config = config.clone();
    new_config.model = model;
    if let Some(p) = provider {
        new_config.provider = p;
    }
    match engine.rebuild(new_config, auth_store.clone()).await {
        Ok(rebuilt) => (RpcResponse::success(req_id, "set_model", None), Some(rebuilt)),
        Err(e) => (RpcResponse::failure(req_id, "set_model", &e.to_string()), None),
    }
}

async fn handle_turn_prompt<W: tokio::io::AsyncWrite + Unpin>(
    (writer, engine, presenter): (
        &mut JsonLinesWriter<W>,
        &rho_engine::engine::AgentEngine,
        &Arc<dyn rho_harness_core::presentation::Presenter>,
    ),
    (message, req_id, op): (&str, Option<String>, &'static str),
) -> Result<()> {
    writer.write_message(&RpcResponse::success(req_id, op, None)).await?;
    let req = crate::engine::runner::TurnRequest::new(message);
    let _ = engine.run_turn(req, presenter.clone()).await;
    Ok(())
}

async fn handle_abort_command<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    writer: &mut JsonLinesWriter<W>,
    engine: &rho_engine::engine::AgentEngine,
) -> Result<()> {
    let _ = engine.record_cancellation("rpc abort").await;
    writer.write_message(&RpcResponse::success(req_id, "abort", None)).await
}

async fn handle_turn_control_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    (writer, engine): (&mut JsonLinesWriter<W>, &rho_engine::engine::AgentEngine),
) -> Result<bool> {
    if matches!(cmd, RpcCommand::Abort) {
        handle_abort_command(req_id, writer, engine).await?;
        return Ok(true);
    }
    if matches!(cmd, RpcCommand::ToolResponse { .. }) {
        writer
            .write_message(&RpcResponse::success(req_id, "tool_response", None))
            .await?;
        return Ok(true);
    }
    Ok(false)
}

fn prompt_command_payload(cmd: &RpcCommand) -> Option<(&str, &'static str)> {
    match cmd {
        RpcCommand::Prompt { message, .. } => Some((message.as_str(), "prompt")),
        RpcCommand::Steer { message } => Some((message.as_str(), "steer")),
        _ => None,
    }
}

async fn handle_turn_prompt_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    io: (
        &mut JsonLinesWriter<W>,
        &rho_engine::engine::AgentEngine,
        &Arc<dyn rho_harness_core::presentation::Presenter>,
    ),
) -> Result<bool> {
    let Some((msg, op)) = prompt_command_payload(cmd) else {
        return Ok(false);
    };
    handle_turn_prompt(io, (msg, req_id, op)).await?;
    Ok(true)
}

async fn handle_turn_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    io: (
        &mut JsonLinesWriter<W>,
        &rho_engine::engine::AgentEngine,
        &Arc<dyn rho_harness_core::presentation::Presenter>,
    ),
) -> Result<bool> {
    if handle_turn_prompt_command(cmd, req_id.clone(), (&mut *io.0, io.1, io.2)).await? {
        return Ok(true);
    }
    handle_turn_control_command(cmd, req_id, (io.0, io.1)).await
}

async fn handle_state_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    (writer, session_id, config): (&mut JsonLinesWriter<W>, &str, &Config),
) -> Result<bool> {
    if matches!(cmd, RpcCommand::GetState) {
        let data = serde_json::json!({
            "session_id": session_id,
            "model": config.model,
            "provider": config.provider,
        });
        writer
            .write_message(&RpcResponse::success(req_id, "get_state", Some(data)))
            .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn execute_set_model<W: tokio::io::AsyncWrite + Unpin>(
    ctx: &mut RpcDaemonContext<'_, W>,
    (model, provider, req_id): (String, Option<String>, Option<String>),
) -> Result<()> {
    let (res, rebuilt) =
        handle_set_model_cmd(ctx.engine, (ctx.config, ctx.auth_store), (model, provider, req_id)).await;
    if let Some(r) = rebuilt {
        *ctx.engine = r;
    }
    ctx.writer.write_message(&res).await
}

async fn handle_config_update_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    match cmd {
        RpcCommand::Compact { instructions } => {
            let res = handle_compact_cmd(ctx.engine, instructions, req_id).await;
            ctx.writer.write_message(&res).await?;
        }
        RpcCommand::SetModel { model, provider } => {
            execute_set_model(ctx, (model, provider, req_id)).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn check_exit_command<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    writer: &mut JsonLinesWriter<W>,
) -> Result<bool> {
    if matches!(cmd, RpcCommand::Exit) {
        writer
            .write_message(&RpcResponse::success(req_id, "exit", None))
            .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn handle_state_or_config<W: tokio::io::AsyncWrite + Unpin>(
    (cmd, req_id): (RpcCommand, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let sid = ctx.engine.session_manager.session_id.clone();
    if !handle_state_command(&cmd, req_id.clone(), (ctx.writer, &sid, ctx.config)).await? {
        handle_config_update_command(cmd, req_id, ctx).await?;
    }
    Ok(())
}

async fn handle_management_command<W: tokio::io::AsyncWrite + Unpin>(
    (cmd, req_id): (RpcCommand, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction> {
    if check_exit_command(&cmd, req_id.clone(), ctx.writer).await? {
        return Ok(RpcLoopAction::Break);
    }
    handle_state_or_config((cmd, req_id), ctx).await?;
    Ok(RpcLoopAction::Continue)
}

async fn dispatch_rpc<W: tokio::io::AsyncWrite + Unpin>(
    (cmd, req_id): (RpcCommand, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction> {
    if handle_turn_command(&cmd, req_id.clone(), (ctx.writer, ctx.engine, ctx.presenter)).await? {
        return Ok(RpcLoopAction::Continue);
    }
    handle_management_command((cmd, req_id), ctx).await
}

async fn handle_read_request<W: tokio::io::AsyncWrite + Unpin>(
    res: rho_harness_core::error::Result<Option<RpcRequest>>,
    writer: &mut JsonLinesWriter<W>,
) -> Result<Option<RpcRequest>> {
    match res {
        Ok(opt) => Ok(opt),
        Err(e) => {
            writer
                .write_message(&RpcResponse::failure(None, "parse", &e.to_string()))
                .await?;
            Ok(None)
        }
    }
}

async fn process_rpc_request<W: tokio::io::AsyncWrite + Unpin>(
    res: rho_harness_core::error::Result<Option<RpcRequest>>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction> {
    let Some(req) = handle_read_request(res, ctx.writer).await? else {
        return Ok(RpcLoopAction::Break);
    };
    dispatch_rpc((req.command, req.id), ctx).await
}

async fn poll_next_rpc<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
) -> NextRpcItem {
    tokio::select! {
        ev = event_rx.recv() => NextRpcItem::Event(ev),
        req = reader.read_message::<RpcRequest>() => NextRpcItem::Request(req),
    }
}

async fn handle_next_rpc_event<R, W>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match poll_next_rpc(reader, event_rx).await {
        NextRpcItem::Event(Some(event)) => {
            ctx.writer.write_message(&event).await?;
            Ok(RpcLoopAction::Continue)
        }
        NextRpcItem::Event(None) => Ok(RpcLoopAction::Break),
        NextRpcItem::Request(res) => process_rpc_request(res, ctx).await,
    }
}

async fn write_initial_rpc_event<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut JsonLinesWriter<W>,
    session_id: &str,
    config: &Config,
) -> Result<()> {
    let init = RpcEvent::SessionStart {
        session_id: session_id.to_string(),
        model: config.model.clone(),
        provider: config.provider.clone(),
    };
    writer.write_message(&init).await
}

async fn run_rpc_loop<R, W>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    loop {
        let action = handle_next_rpc_event(reader, event_rx, ctx).await?;
        if matches!(action, RpcLoopAction::Break) {
            break;
        }
    }
    Ok(())
}

pub async fn run_rpc_daemon(config: Config, auth_store: AuthStore) -> Result<()> {
    let mut reader = JsonLinesReader::new(BufReader::new(tokio::io::stdin()));
    let mut writer = JsonLinesWriter::new(tokio::io::stdout());
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RpcEvent>();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(RpcPresenter::new(event_tx));
    let mut engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None).await?;

    write_initial_rpc_event(&mut writer, &engine.session_manager.session_id, &config).await?;
    let mut ctx = RpcDaemonContext {
        writer: &mut writer,
        engine: &mut engine,
        presenter: &presenter,
        config: &config,
        auth_store: &auth_store,
    };
    run_rpc_loop(&mut reader, &mut event_rx, &mut ctx).await
}
