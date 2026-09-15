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
    event_tx: mpsc::UnboundedSender<RpcEvent>,
    auth_bridge: rho_harness_core::rpc::RpcAuthBridge,
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

async fn handle_steer_command<W: tokio::io::AsyncWrite + Unpin>(
    (message, req_id): (String, Option<String>),
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    if let Some(steering) = crate::platform::remote::get_active_steering() {
        steering.enqueue(message.clone());
        crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TextChunk {
            content: format!("\n[Steering from remote]: {message}\n"),
        });
    }
    ctx.steering.enqueue(message);
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "steer", None))
        .await
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
    let resp = parse_tool_decision(&decision);
    if let Some(sender) = crate::platform::remote::ACTIVE_APPROVALS
        .lock()
        .unwrap()
        .remove(&approval_id)
    {
        let _ = sender.send(resp.clone());
    }
    let mut approvals = ctx.pending_approvals.lock().await;
    if let Some(sender) = approvals.remove(&approval_id) {
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
    if crate::platform::remote::is_repl_active() {
        if let Some(steering) = crate::platform::remote::get_active_steering() {
            steering.enqueue(message.clone());
            crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TextChunk {
                content: format!("\n[Steering from remote]: {message}\n"),
            });
            ctx.writer
                .write_message(&RpcResponse::success(
                    req_id,
                    "prompt",
                    Some(serde_json::json!({ "steered": true })),
                ))
                .await?;
            return Ok(());
        }

        crate::platform::remote::REMOTE_PROMPT_QUEUE.push(message);
        ctx.writer
            .write_message(&RpcResponse::success(req_id, "prompt", None))
            .await?;
        return Ok(());
    }

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

fn extract_user_chat_messages(
    content: &[rig::message::UserContent],
    pending_tools: &mut std::collections::HashMap<String, serde_json::Value>,
    out: &mut Vec<serde_json::Value>,
) {
    let mut text = String::new();
    for part in content {
        match part {
            rig::message::UserContent::Text(t) => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&t.text);
            }
            rig::message::UserContent::ToolResult(res) => {
                let res_text = res
                    .content
                    .iter()
                    .filter_map(|p| p.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                let tool_info = pending_tools.remove(res.call.as_str());
                let (name, args) = if let Some(ref ti) = tool_info {
                    (
                        ti.get("tool").and_then(|v| v.as_str()).unwrap_or("tool").to_string(),
                        ti.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
                    )
                } else {
                    ("tool".to_string(), serde_json::Value::Null)
                };
                out.push(serde_json::json!({
                    "role": "tool",
                    "tool": name,
                    "arguments": args,
                    "output": res_text,
                    "is_error": false,
                }));
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        out.push(serde_json::json!({
            "role": "user",
            "content": text,
        }));
    }
}

fn extract_assistant_chat_messages(
    content: &[rig::message::AssistantContent],
    pending_tools: &mut std::collections::HashMap<String, serde_json::Value>,
    out: &mut Vec<serde_json::Value>,
) {
    let mut text = String::new();
    for part in content {
        match part {
            rig::message::AssistantContent::Text(t) => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&t.text);
            }
            rig::message::AssistantContent::ToolCall(call) => {
                if !text.is_empty() {
                    out.push(serde_json::json!({
                        "role": "assistant",
                        "content": std::mem::take(&mut text),
                    }));
                }
                pending_tools.insert(
                    call.id.to_string(),
                    serde_json::json!({
                        "tool": call.function.name,
                        "arguments": call.function.arguments,
                    }),
                );
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        out.push(serde_json::json!({
            "role": "assistant",
            "content": text,
        }));
    }
}

fn extract_chat_messages(messages: &[rig::message::Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut pending_tools = std::collections::HashMap::new();

    for msg in messages {
        match msg {
            rig::message::Message::User { content } => {
                extract_user_chat_messages(content, &mut pending_tools, &mut out);
            }
            rig::message::Message::Assistant { content, .. } => {
                extract_assistant_chat_messages(content, &mut pending_tools, &mut out);
            }
            _ => {}
        }
    }
    out
}

async fn handle_resume_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
    session_id: &str,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let cfg = ctx.config.read().await.clone();
    let auth = ctx.auth_store.read().await.clone();
    let base_dir = ctx.engine.read().await.base_dir.clone();
    match crate::platform::agent_engine_in_dir(cfg, auth, base_dir, Some(session_id)).await {
        Ok(new_eng) => {
            let sid = new_eng.session_manager.session_id.clone();
            let raw_msgs = new_eng.session_manager.load_messages().await.unwrap_or_default();
            let messages = extract_chat_messages(&raw_msgs);
            new_eng.refresh_quota().await;
            let totals = new_eng.session_usage_totals();
            let active_branch = rho_ui_core::footer::get_git_branch(&new_eng.base_dir);
            let payload = serde_json::json!({
                "session_id": sid,
                "messages": messages,
                "active_workspace": new_eng.base_dir.display().to_string(),
                "active_branch": active_branch,
                "quota": new_eng.quota_display(),
                "total_input_tokens": totals.total_input,
                "total_output_tokens": totals.total_output,
                "total_cache_read_tokens": totals.total_cache_read,
                "total_cache_write_tokens": totals.total_cache_write,
                "total_cost": serde_json::Value::Null,
                "context_percent": new_eng.context_percent_f64(),
                "context_window": new_eng.context_limit().unwrap_or(0),
                "tokens_per_second": new_eng.tokens_per_second(),
            });
            *ctx.engine.write().await = new_eng;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "resume_session", Some(payload)))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "resume_session", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

async fn handle_fork_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
    node_id: Option<&str>,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let cfg = ctx.config.read().await;
    let eng = ctx.engine.read().await;
    match eng.session_manager.fork_session(&cfg.sessions_dir, node_id).await {
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
    Ok(())
}

async fn handle_resume_or_fork_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<bool> {
    match cmd {
        RpcCommand::ResumeSession { session_id } => {
            handle_resume_session_cmd(session_id, req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::ForkSession { node_id } => {
            handle_fork_session_cmd(node_id.as_deref(), req_id, ctx).await?;
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
    eng.refresh_quota().await;
    let totals = eng.session_usage_totals();
    let approvals = ctx.pending_approvals.lock().await;
    let status = if !approvals.is_empty() {
        "waiting_approval"
    } else if ctx.active_turn.is_some() {
        "busy"
    } else {
        "idle"
    };

    let raw_msgs = eng.session_manager.load_messages().await.unwrap_or_default();
    let messages = extract_chat_messages(&raw_msgs);
    let active_branch = rho_ui_core::footer::get_git_branch(&eng.base_dir);

    let data = serde_json::json!({
        "session_id": eng.session_manager.session_id,
        "model": cfg.model,
        "provider": cfg.provider,
        "thinking_level": cfg.thinking_level,
        "status": status,
        "messages": messages,
        "active_workspace": eng.base_dir.display().to_string(),
        "active_branch": active_branch,
        "quota": eng.quota_display(),
        "total_input_tokens": totals.total_input,
        "total_output_tokens": totals.total_output,
        "total_cache_read_tokens": totals.total_cache_read,
        "total_cache_write_tokens": totals.total_cache_write,
        "total_cost": serde_json::Value::Null,
        "context_percent": eng.context_percent_f64(),
        "context_window": eng.context_limit().unwrap_or(0),
        "tokens_per_second": eng.tokens_per_second(),
    });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "get_state", Some(data)))
        .await
}

fn get_host_name() -> String {
    if let Ok(name) = std::env::var("HOSTNAME") {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

async fn handle_node_info_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let hostname = get_host_name();
    let (active_workspace, active_branch) = {
        let eng = ctx.engine.read().await;
        let ws = eng.base_dir.display().to_string();
        let branch = rho_ui_core::footer::get_git_branch(&eng.base_dir);
        (Some(ws), branch)
    };
    let status = if ctx.active_turn.is_some() { "busy" } else { "idle" };
    let payload = serde_json::json!({
        "hostname": hostname,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
        "active_workspace": active_workspace,
        "active_branch": active_branch,
        "status": status,
    });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "get_node_info", Some(payload)))
        .await?;
    Ok(())
}

async fn handle_create_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
    workspace: Option<String>,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut cfg = ctx.config.read().await.clone();
    let current_base = ctx.engine.read().await.base_dir.clone();
    let target_dir = if let Some(ws) = workspace {
        let path = std::path::PathBuf::from(ws);
        if path.is_absolute() && path.exists() {
            path
        } else {
            let resolved = current_base.join(path);
            if resolved.exists() { resolved } else { current_base }
        }
    } else {
        current_base
    };
    cfg.sessions_dir = target_dir.join(".rho/sessions");

    let auth = ctx.auth_store.read().await.clone();
    match crate::platform::agent_engine_in_dir(cfg.clone(), auth, target_dir, None).await {
        Ok(new_eng) => {
            let session_id = new_eng.session_manager.session_id.clone();
            let base_dir = new_eng.base_dir.display().to_string();
            let active_branch = rho_ui_core::footer::get_git_branch(&new_eng.base_dir);
            new_eng.refresh_quota().await;
            let totals = new_eng.session_usage_totals();
            let payload = serde_json::json!({
                "session_id": session_id,
                "workspace": base_dir,
                "active_workspace": base_dir,
                "active_branch": active_branch,
                "quota": new_eng.quota_display(),
                "total_input_tokens": totals.total_input,
                "total_output_tokens": totals.total_output,
                "total_cache_read_tokens": totals.total_cache_read,
                "total_cache_write_tokens": totals.total_cache_write,
                "total_cost": serde_json::Value::Null,
                "context_percent": new_eng.context_percent_f64(),
                "context_window": new_eng.context_limit().unwrap_or(0),
                "tokens_per_second": new_eng.tokens_per_second(),
            });
            *ctx.engine.write().await = new_eng;
            *ctx.config.write().await = cfg;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "create_session", Some(payload)))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "create_session", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

async fn handle_auth_login_cmd<W: tokio::io::AsyncWrite + Unpin>(
    provider_str: String,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let provider_id = match provider_str.parse::<rho_harness_core::provider::ProviderId>() {
        Ok(p) => p,
        Err(_) => {
            ctx.writer
                .write_message(&RpcResponse::failure(
                    req_id,
                    "auth_login",
                    &format!("Unknown provider '{provider_str}'"),
                ))
                .await?;
            return Ok(());
        }
    };
    let callbacks = ctx.auth_bridge.callbacks(&provider_str, ctx.event_tx.clone());
    let auth_store = Arc::clone(&ctx.auth_store);
    let event_tx = ctx.event_tx.clone();
    let prov = provider_str.clone();

    tokio::spawn(async move {
        match rho_engine::auth::perform_oauth_login(provider_id, &callbacks).await {
            Ok(cred) => {
                let mut store = auth_store.write().await;
                let _ = store.set_credential_async(&prov, cred).await;
                let _ = store.save_async().await;
                let _ = event_tx.send(RpcEvent::AuthComplete {
                    provider: prov,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                let _ = event_tx.send(RpcEvent::AuthComplete {
                    provider: prov,
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    });

    ctx.writer
        .write_message(&RpcResponse::success(req_id, "auth_login", None))
        .await?;
    Ok(())
}

async fn handle_auth_input_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (interaction_id, secret_value, selected_option): (String, Option<String>, Option<String>),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let resolved = ctx.auth_bridge.resolve_input(
        &interaction_id,
        rho_harness_core::rpc::AuthInputResponse {
            secret_value,
            selected_option,
        },
    );
    let payload = serde_json::json!({ "resolved": resolved });
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "auth_input", Some(payload)))
        .await?;
    Ok(())
}

async fn handle_set_api_key_cmd<W: tokio::io::AsyncWrite + Unpin>(
    (provider, api_key): (String, String),
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let mut store = ctx.auth_store.write().await;
    match store.set_api_key_async(&provider, api_key).await {
        Ok(_) => {
            let _ = store.save_async().await;
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "set_api_key", None))
                .await?;
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "set_api_key", &e.to_string()))
                .await?;
        }
    }
    Ok(())
}

async fn handle_remote_auth_cmd<W: tokio::io::AsyncWrite + Unpin>(
    cmd: &RpcCommand,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<bool> {
    match cmd {
        RpcCommand::GetNodeInfo => {
            handle_node_info_cmd(req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::CreateSession { workspace } => {
            handle_create_session_cmd(workspace.clone(), req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::AuthLogin { provider } => {
            handle_auth_login_cmd(provider.clone(), req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::AuthInput {
            interaction_id,
            secret_value,
            selected_option,
        } => {
            handle_auth_input_cmd(
                (interaction_id.clone(), secret_value.clone(), selected_option.clone()),
                req_id,
                ctx,
            )
            .await?;
            Ok(true)
        }
        RpcCommand::SetApiKey { provider, api_key } => {
            handle_set_api_key_cmd((provider.clone(), api_key.clone()), req_id, ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
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
            handle_steer_command((message, req_id), ctx).await?;
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
        ref other if handle_remote_auth_cmd(other, req_id.clone(), ctx).await? => {}
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
    let eng = ctx.engine.read().await;
    eng.refresh_quota().await;
    let totals = eng.session_usage_totals();
    let usage_ev = RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: eng.context_percent_f64(),
        context_window: eng.context_limit(),
        tokens_per_second: eng.tokens_per_second(),
        quota: eng.quota_display(),
    };
    ctx.writer.write_message(&usage_ev).await?;
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

pub async fn run_rpc_session_over_stream<R, W>(
    reader_stream: R,
    writer_stream: W,
    engine_lock: Arc<RwLock<rho_engine::engine::AgentEngine>>,
    config_lock: Arc<RwLock<Config>>,
    auth_store_lock: Arc<RwLock<AuthStore>>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut reader = JsonLinesReader::new(BufReader::new(reader_stream));
    let mut writer = JsonLinesWriter::new(writer_stream);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RpcEvent>();
    crate::platform::remote::PEER_REGISTRY.register(event_tx.clone());
    let rpc_presenter = RpcPresenter::new(event_tx.clone());
    let pending_approvals = rpc_presenter.pending_approvals();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(rpc_presenter);

    let (session_id, steering_mode) = {
        let eng = engine_lock.read().await;
        (eng.session_manager.session_id.clone(), eng.config.steering_mode)
    };
    let (model, provider) = {
        let cfg = config_lock.read().await;
        (cfg.model.clone(), cfg.provider.clone())
    };

    let steering = Arc::new(SharedSteeringQueue::new(steering_mode));

    let init = RpcEvent::SessionStart {
        session_id,
        model,
        provider,
    };
    writer.write_message(&init).await?;

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
        event_tx,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
    };
    run_rpc_loop(&mut reader, &mut event_rx, &mut ctx).await
}

pub async fn run_rpc_daemon(config: Config, auth_store: AuthStore) -> Result<()> {
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None).await?;
    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));
    run_rpc_session_over_stream(
        tokio::io::stdin(),
        tokio::io::stdout(),
        engine_lock,
        config_lock,
        auth_store_lock,
    )
    .await
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

        let (test_event_tx, _test_event_rx) = mpsc::unbounded_channel();
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
            event_tx: test_event_tx,
            auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
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

        let (client_io, server_io) = duplex(4096);
        let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
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

        let (test_event_tx, _test_event_rx) = mpsc::unbounded_channel();
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
            event_tx: test_event_tx,
            auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
        };

        handle_state_command(Some("req-state".to_string()), &mut ctx)
            .await
            .unwrap();
        handle_get_tree_cmd(Some("req-tree".to_string()), &mut ctx)
            .await
            .unwrap();
        handle_node_info_cmd(Some("req-info".to_string()), &mut ctx)
            .await
            .unwrap();
        handle_create_session_cmd(
            Some(temp_dir.display().to_string()),
            Some("req-create".to_string()),
            &mut ctx,
        )
        .await
        .unwrap();

        let resp1: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
        assert_eq!(resp1.id, Some("req-state".to_string()));
        let state_data = resp1.data.unwrap();
        assert!(state_data.get("active_workspace").is_some());
        assert!(state_data.get("total_input_tokens").is_some());
        assert!(state_data.get("context_window").is_some());

        let _resp2: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
        let _resp3: RpcResponse = client_reader.read_message().await.unwrap().unwrap();

        let resp4: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
        assert_eq!(resp4.id, Some("req-create".to_string()));
        let create_data = resp4.data.unwrap();
        assert!(create_data.get("active_workspace").is_some());
        assert!(create_data.get("total_input_tokens").is_some());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_extract_chat_messages_preserves_tools() {
        use rig::message::{
            AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
        };
        let msgs = vec![
            Message::user("run check"),
            Message::Assistant {
                id: None,
                content: vec![
                    AssistantContent::text("I will run the command."),
                    AssistantContent::ToolCall(ToolCall::new(
                        ToolCallId::new_or_mint("call_1"),
                        ToolFunction::new("bash".to_string(), serde_json::json!({ "command": "cargo check" })),
                    )),
                ],
            },
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: ToolCallId::new_or_mint("call_1"),
                    provider: None,
                    name: "bash".to_string(),
                    content: vec![ToolResultContent::Text(rig::message::Text::new(
                        "Finished dev [unoptimized + debuginfo]",
                    ))],
                })],
            },
            Message::assistant("Check passed cleanly."),
        ];

        let extracted = extract_chat_messages(&msgs);
        assert_eq!(extracted.len(), 4);
        assert_eq!(extracted[0]["role"], "user");
        assert_eq!(extracted[0]["content"], "run check");
        assert_eq!(extracted[1]["role"], "assistant");
        assert_eq!(extracted[1]["content"], "I will run the command.");
        assert_eq!(extracted[2]["role"], "tool");
        assert_eq!(extracted[2]["tool"], "bash");
        assert_eq!(extracted[2]["arguments"]["command"], "cargo check");
        assert_eq!(extracted[2]["output"], "Finished dev [unoptimized + debuginfo]");
        assert_eq!(extracted[3]["role"], "assistant");
        assert_eq!(extracted[3]["content"], "Check passed cleanly.");
    }
}
