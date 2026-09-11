use std::sync::Arc;
use tokio::io::BufReader;
use tokio::sync::{RwLock, mpsc};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::render::RpcPresenter;
use crate::ui::render::rpc_presenter::PendingApprovals;
use rho_engine::engine::runner::TurnOutput;
use rho_harness_core::presentation::types::InteractionResponse;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};

enum RpcLoopAction {
    Continue,
    Break,
}

enum NextRpcItem {
    Event(Option<RpcEvent>),
    Request(rho_harness_core::error::Result<Option<RpcRequest>>),
    TurnDone(Box<std::result::Result<Result<TurnOutput>, tokio::task::JoinError>>),
}

struct RpcDaemonContext<'a, W> {
    writer: &'a mut JsonLinesWriter<W>,
    engine: Arc<RwLock<rho_engine::engine::AgentEngine>>,
    presenter: Arc<dyn rho_harness_core::presentation::Presenter>,
    config: Arc<RwLock<Config>>,
    auth_store: Arc<RwLock<AuthStore>>,
    pending_approvals: PendingApprovals,
    steering: Arc<SharedSteeringQueue>,
    active_turn: &'a mut Option<tokio::task::JoinHandle<Result<TurnOutput>>>,
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

async fn handle_abort_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if let Some(handle) = ctx.active_turn.take() {
        handle.abort();
    }
    ctx.steering.clear();
    let eng = ctx.engine.read().await;
    let _ = eng.record_cancellation("rpc abort").await;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "idle".to_string(),
        })
        .await?;
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "abort", None))
        .await
}

fn parse_tool_decision(decision: &str) -> InteractionResponse {
    let lower = decision.trim().to_lowercase();
    match lower.as_str() {
        "allow" | "0" => InteractionResponse::Selected(0),
        "always" | "always_allow" | "2" => InteractionResponse::Selected(2),
        "deny" | "denied" | "cancel" | "cancelled" => InteractionResponse::Cancelled,
        d if d.starts_with("edit:") => InteractionResponse::SelectedWithInput {
            index: 1,
            text: decision.trim()[5..].trim().to_string(),
        },
        d if d.starts_with("deny:") => InteractionResponse::SelectedWithInput {
            index: 3,
            text: decision.trim()[5..].trim().to_string(),
        },
        _ => InteractionResponse::Selected(0),
    }
}

async fn handle_tool_response_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (approval_id, decision, req_id): (String, String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut approvals = ctx.pending_approvals.lock().await;
    if let Some(sender) = approvals.remove(&approval_id) {
        let resp = parse_tool_decision(&decision);
        let _ = sender.send(resp);
    }
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "tool_response", None))
        .await
}

async fn handle_prompt_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (message, req_id): (String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if ctx.active_turn.is_some() {
        ctx.writer
            .write_message(&RpcResponse::failure(
                req_id,
                "prompt",
                "A turn is already in progress. Steer or abort the active turn first.",
            ))
            .await?;
        return Ok(());
    }

    ctx.writer
        .write_message(&RpcResponse::success(req_id, "prompt", None))
        .await?;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "busy".to_string(),
        })
        .await?;

    let engine = Arc::clone(&ctx.engine);
    let presenter = Arc::clone(&ctx.presenter);
    let steering = Arc::clone(&ctx.steering);
    let handle = tokio::spawn(async move {
        let req = crate::engine::runner::TurnRequest::new(&message).with_steering(steering);
        let eng = engine.read().await;
        eng.run_turn(req, presenter).await
    });
    *ctx.active_turn = Some(handle);
    Ok(())
}

async fn handle_get_tree_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let eng = ctx.engine.read().await;
    match eng.session_manager.load_tree().await {
        Ok(tree) => {
            let entries = crate::ui::interactive::tree_view::build_tree_display(&tree);
            let payload = serde_json::json!({
                "active_leaf_id": tree.active_leaf_id,
                "session_name": tree.session_name,
                "entries": entries.into_iter().map(|e| serde_json::json!({
                    "id": e.id,
                    "parent_id": e.parent_id,
                    "depth": e.depth,
                    "is_active": e.is_active,
                    "label": e.label,
                    "preview": e.preview,
                    "kind": format!("{:?}", e.kind),
                })).collect::<Vec<_>>(),
            });
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "get_tree", Some(payload)))
                .await
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "get_tree", &e.to_string()))
                .await
        }
    }
}

async fn handle_session_lifecycle_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<bool> {
    match cmd {
        RpcCommand::GetTree => {
            handle_get_tree_cmd(req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::SwitchBranch { node_id } => {
            let eng = ctx.engine.read().await;
            match eng.session_manager.switch_branch(Some(node_id.clone())).await {
                Ok(_) => {
                    ctx.writer
                        .write_message(&RpcResponse::success(req_id, "switch_branch", None))
                        .await?;
                }
                Err(e) => {
                    ctx.writer
                        .write_message(&RpcResponse::failure(req_id, "switch_branch", &e.to_string()))
                        .await?;
                }
            }
            Ok(true)
        }
        RpcCommand::SetNodeLabel { node_id, label } => {
            let eng = ctx.engine.read().await;
            let _ = eng.session_manager.set_node_label(node_id, label.clone()).await;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "set_node_label", None))
                .await?;
            Ok(true)
        }
        RpcCommand::ListSessions => {
            let cfg = ctx.config.read().await;
            let summaries = rho_harness_core::session::list_session_summaries(&cfg.sessions_dir).unwrap_or_default();
            ctx.writer
                .write_message(&RpcResponse::success(
                    req_id,
                    "list_sessions",
                    Some(serde_json::to_value(summaries).unwrap_or_default()),
                ))
                .await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn handle_resume_or_fork_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<bool> {
    match cmd {
        RpcCommand::ResumeSession { session_id } => {
            let cfg = ctx.config.read().await.clone();
            let auth = ctx.auth_store.read().await.clone();
            match crate::platform::agent_engine(cfg, auth, Some(session_id)).await {
                Ok(new_eng) => {
                    *ctx.engine.write().await = new_eng;
                    ctx.writer
                        .write_message(&RpcResponse::success(req_id, "resume_session", None))
                        .await?;
                }
                Err(e) => {
                    ctx.writer
                        .write_message(&RpcResponse::failure(req_id, "resume_session", &e.to_string()))
                        .await?;
                }
            }
            Ok(true)
        }
        RpcCommand::ForkSession { node_id } => {
            let cfg = ctx.config.read().await;
            let eng = ctx.engine.read().await;
            match eng
                .session_manager
                .fork_session(&cfg.sessions_dir, node_id.as_deref())
                .await
            {
                Ok(forked) => {
                    let data = serde_json::json!({ "session_id": forked.session_id });
                    ctx.writer
                        .write_message(&RpcResponse::success(req_id, "fork_session", Some(data)))
                        .await?;
                }
                Err(e) => {
                    ctx.writer
                        .write_message(&RpcResponse::failure(req_id, "fork_session", &e.to_string()))
                        .await?;
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn handle_state_command<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let eng = ctx.engine.read().await;
    let cfg = ctx.config.read().await;
    let approvals = ctx.pending_approvals.lock().await;
    let status = if !approvals.is_empty() {
        "waiting_approval"
    } else if ctx.active_turn.is_some() {
        "busy"
    } else {
        "idle"
    };

    let data = serde_json::json!({
        "session_id": eng.session_manager.session_id,
        "model": cfg.model,
        "provider": cfg.provider,
        "thinking_level": cfg.thinking_level,
        "status": status,
    });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "get_state", Some(data)))
        .await
}

async fn handle_set_model_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (model, provider): (String, Option<String>),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut cfg = ctx.config.write().await;
    cfg.model = model;
    if let Some(p) = provider {
        cfg.provider = p;
    }
    let auth = ctx.auth_store.read().await.clone();
    let mut eng = ctx.engine.write().await;
    match eng.rebuild(cfg.clone(), auth).await {
        Ok(rebuilt) => {
            *eng = rebuilt;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "set_model", None))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "set_model", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

async fn handle_set_thinking_cmd<W: tokio::io::AsyncWrite + Unpin>(
    level: String,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut cfg = ctx.config.write().await;
    cfg.thinking_level = Some(level.clone());
    let mut eng = ctx.engine.write().await;
    eng.config.thinking_level = Some(level);
    let _ = eng.update_model().await;
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "set_thinking", None))
        .await
}

async fn handle_config_update_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    match cmd {
        RpcCommand::Compact { instructions } => {
            let eng = ctx.engine.read().await;
            let res = handle_compact_cmd(&eng, instructions, req_id).await;
            ctx.writer.write_message(&res).await?;
        }
        RpcCommand::SetModel { model, provider } => {
            handle_set_model_cmd((model, provider), req_id, ctx).await?;
        }
        RpcCommand::SetThinking { level } => {
            handle_set_thinking_cmd(level, req_id, ctx).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn dispatch_rpc<W: tokio::io::AsyncWrite + Unpin>(
    (cmd, req_id): (RpcCommand, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction> {
    match cmd {
        RpcCommand::Prompt { message, .. } => {
            handle_prompt_cmd((message, req_id), ctx).await?;
        }
        RpcCommand::Steer { message } => {
            ctx.steering.enqueue(message);
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "steer", None))
                .await?;
        }
        RpcCommand::Abort => {
            handle_abort_cmd(req_id, ctx).await?;
        }
        RpcCommand::ToolResponse { approval_id, decision } => {
            handle_tool_response_cmd((approval_id, decision, req_id), ctx).await?;
        }
        RpcCommand::GetState => {
            handle_state_command(req_id, ctx).await?;
        }
        RpcCommand::Exit => {
            if let Some(handle) = ctx.active_turn.take() {
                handle.abort();
            }
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "exit", None))
                .await?;
            return Ok(RpcLoopAction::Break);
        }
        ref other if handle_session_lifecycle_cmd(other, req_id.clone(), ctx).await? => {}
        ref other if handle_resume_or_fork_cmd(other, req_id.clone(), ctx).await? => {}
        other => {
            handle_config_update_cmd(other, req_id, ctx).await?;
        }
    }
    Ok(RpcLoopAction::Continue)
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

async fn poll_next_rpc<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    active_turn: &mut Option<tokio::task::JoinHandle<Result<TurnOutput>>>,
) -> NextRpcItem {
    if let Some(turn) = active_turn.as_mut() {
        tokio::select! {
            ev = event_rx.recv() => NextRpcItem::Event(ev),
            res = turn => NextRpcItem::TurnDone(Box::new(res)),
            req = reader.read_message::<RpcRequest>() => NextRpcItem::Request(req),
        }
    } else {
        tokio::select! {
            ev = event_rx.recv() => NextRpcItem::Event(ev),
            req = reader.read_message::<RpcRequest>() => NextRpcItem::Request(req),
        }
    }
}

async fn handle_turn_done<W: tokio::io::AsyncWrite + Unpin>(
    ctx: &mut RpcDaemonContext<'_, W>,
    res: std::result::Result<Result<TurnOutput>, tokio::task::JoinError>,
) -> Result<()> {
    *ctx.active_turn = None;
    ctx.writer
        .write_message(&RpcEvent::StatusChanged {
            status: "idle".to_string(),
        })
        .await?;
    if let Ok(Err(err)) = res {
        ctx.writer
            .write_message(&RpcEvent::Error {
                code: "turn_error".to_string(),
                message: err.to_string(),
            })
            .await?;
    }
    Ok(())
}

async fn handle_next_rpc_item<R, W>(
    reader: &mut JsonLinesReader<R>,
    event_rx: &mut mpsc::UnboundedReceiver<RpcEvent>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<RpcLoopAction>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match poll_next_rpc(reader, event_rx, ctx.active_turn).await {
        NextRpcItem::Event(Some(event)) => {
            ctx.writer.write_message(&event).await?;
            Ok(RpcLoopAction::Continue)
        }
        NextRpcItem::Event(None) => Ok(RpcLoopAction::Break),
        NextRpcItem::TurnDone(res) => {
            handle_turn_done(ctx, *res).await?;
            Ok(RpcLoopAction::Continue)
        }
        NextRpcItem::Request(res) => {
            let Some(req) = handle_read_request(res, ctx.writer).await? else {
                return Ok(RpcLoopAction::Break);
            };
            dispatch_rpc((req.command, req.id), ctx).await
        }
    }
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
        let action = handle_next_rpc_item(reader, event_rx, ctx).await?;
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
    let rpc_presenter = RpcPresenter::new(event_tx);
    let pending_approvals = rpc_presenter.pending_approvals();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(rpc_presenter);
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None).await?;
    let steering = Arc::new(SharedSteeringQueue::new(engine.config.steering_mode));

    let init = RpcEvent::SessionStart {
        session_id: engine.session_manager.session_id.clone(),
        model: config.model.clone(),
        provider: config.provider.clone(),
    };
    writer.write_message(&init).await?;

    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));
    let mut active_turn = None;

    let mut ctx = RpcDaemonContext {
        writer: &mut writer,
        engine: engine_lock,
        presenter,
        config: config_lock,
        auth_store: auth_store_lock,
        pending_approvals,
        steering,
        active_turn: &mut active_turn,
    };
    run_rpc_loop(&mut reader, &mut event_rx, &mut ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::presentation::presenter::Presenter;
    use rho_harness_core::presentation::{InteractionOption, InteractionPrompt, OptionLayout};
    use tokio::io::duplex;

    #[test]
    fn test_parse_tool_decision_mappings() {
        assert_eq!(parse_tool_decision("allow"), InteractionResponse::Selected(0));
        assert_eq!(parse_tool_decision("0"), InteractionResponse::Selected(0));
        assert_eq!(parse_tool_decision("always"), InteractionResponse::Selected(2));
        assert_eq!(parse_tool_decision("always_allow"), InteractionResponse::Selected(2));
        assert_eq!(parse_tool_decision("deny"), InteractionResponse::Cancelled);
        assert_eq!(parse_tool_decision("cancel"), InteractionResponse::Cancelled);
        assert_eq!(
            parse_tool_decision("edit: echo hello"),
            InteractionResponse::SelectedWithInput {
                index: 1,
                text: "echo hello".to_string(),
            }
        );
        assert_eq!(
            parse_tool_decision("deny: security policy violation"),
            InteractionResponse::SelectedWithInput {
                index: 3,
                text: "security policy violation".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_rpc_presenter_tool_approval_roundtrip() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let presenter = RpcPresenter::new(event_tx);
        let pending = presenter.pending_approvals();

        let prompt = InteractionPrompt {
            title: "bash".to_string(),
            body: "rm -rf target".to_string(),
            options: vec![InteractionOption {
                label: "Allow".to_string(),
                description: Some("Run command".to_string()),
                input: None,
            }],
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: OptionLayout::Vertical,
        };

        let pres_clone = presenter.clone();
        let handle = tokio::spawn(async move { pres_clone.request_interaction(prompt).await });

        // 1. First event: ToolApprovalRequest
        let ev1 = event_rx.recv().await.expect("expected tool approval event");
        let approval_id = match ev1 {
            RpcEvent::ToolApprovalRequest { approval_id, tool, .. } => {
                assert_eq!(tool, "bash");
                approval_id
            }
            other => panic!("expected ToolApprovalRequest, got {other:?}"),
        };

        // 2. Second event: StatusChanged { status: "waiting_approval" }
        let ev2 = event_rx.recv().await.expect("expected status change");
        assert_eq!(
            ev2,
            RpcEvent::StatusChanged {
                status: "waiting_approval".to_string()
            }
        );

        // 3. Resolve approval via handle_tool_response_cmd
        let (_client_io, server_io) = duplex(1024);
        let mut writer = JsonLinesWriter::new(server_io);
        let temp_dir = std::env::temp_dir().join(format!("rpc_test_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: temp_dir.join("sessions"),
            auth_file: temp_dir.join("auth.json"),
            ..Config::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None)
            .await
            .unwrap();
        let engine_lock = Arc::new(RwLock::new(engine));
        let config_lock = Arc::new(RwLock::new(config));
        let auth_store_lock = Arc::new(RwLock::new(auth_store));
        let steering = Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All));
        let mut active_turn = None;

        let pres_arc: Arc<dyn Presenter> = Arc::new(presenter);
        let mut ctx = RpcDaemonContext {
            writer: &mut writer,
            engine: engine_lock,
            presenter: pres_arc,
            config: config_lock,
            auth_store: auth_store_lock,
            pending_approvals: pending,
            steering,
            active_turn: &mut active_turn,
        };

        handle_tool_response_cmd((approval_id, "allow".to_string(), None), &mut ctx)
            .await
            .unwrap();

        let response = handle.await.unwrap();
        assert_eq!(response, Some(InteractionResponse::Selected(0)));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_rpc_state_command_and_get_tree() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let presenter = RpcPresenter::new(event_tx);
        let pending = presenter.pending_approvals();

        let (_client_io, server_io) = duplex(4096);
        let mut writer = JsonLinesWriter::new(server_io);
        let temp_dir = std::env::temp_dir().join(format!("rpc_tree_test_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: temp_dir.join("sessions"),
            auth_file: temp_dir.join("auth.json"),
            model: "mock-model".to_string(),
            provider: "ollama".to_string(),
            ..Config::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None)
            .await
            .unwrap();
        let engine_lock = Arc::new(RwLock::new(engine));
        let config_lock = Arc::new(RwLock::new(config));
        let auth_store_lock = Arc::new(RwLock::new(auth_store));
        let steering = Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All));
        let mut active_turn = None;

        let pres_arc: Arc<dyn Presenter> = Arc::new(presenter);
        let mut ctx = RpcDaemonContext {
            writer: &mut writer,
            engine: engine_lock,
            presenter: pres_arc,
            config: config_lock,
            auth_store: auth_store_lock,
            pending_approvals: pending,
            steering,
            active_turn: &mut active_turn,
        };

        handle_state_command(Some("req-state".to_string()), &mut ctx)
            .await
            .unwrap();
        handle_get_tree_cmd(Some("req-tree".to_string()), &mut ctx)
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
