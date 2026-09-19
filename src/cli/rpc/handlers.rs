use std::sync::Arc;

use super::types::RpcDaemonContext;
use crate::error::Result;
use rho_harness_core::presentation::types::InteractionResponse;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcResponse};

pub(crate) async fn handle_compact_cmd(
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

pub(crate) async fn handle_steer_command<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_abort_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) fn parse_tool_decision(decision: &str) -> InteractionResponse {
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

pub(crate) async fn handle_tool_response_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_prompt_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_get_tree_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_session_lifecycle_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) fn extract_user_chat_messages(
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

pub(crate) fn extract_assistant_chat_messages(
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

pub(crate) fn extract_chat_messages(messages: &[rig::message::Message]) -> Vec<serde_json::Value> {
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

pub(crate) async fn handle_resume_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
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
            let active_branch = crate::ui::interactive::footer::path::get_git_branch(&new_eng.base_dir);
            let payload = serde_json::json!({
                "session_id": sid,
                "model": new_eng.config.model,
                "provider": new_eng.config.provider,
                "thinking_level": new_eng.config.thinking_level,
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

pub(crate) async fn handle_fork_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_resume_or_fork_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_state_command<W: tokio::io::AsyncWrite + Unpin>(
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
    let active_branch = crate::ui::interactive::footer::path::get_git_branch(&eng.base_dir);

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

pub(crate) fn get_host_name() -> String {
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

pub(crate) async fn handle_node_info_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let hostname = get_host_name();
    let (active_workspace, active_branch) = {
        let eng = ctx.engine.read().await;
        let ws = eng.base_dir.display().to_string();
        let branch = crate::ui::interactive::footer::path::get_git_branch(&eng.base_dir);
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

pub(crate) async fn handle_create_session_cmd<W: tokio::io::AsyncWrite + Unpin>(
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
            let active_branch = crate::ui::interactive::footer::path::get_git_branch(&new_eng.base_dir);
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

pub(crate) async fn handle_auth_login_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_auth_input_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_set_api_key_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_remote_auth_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_set_model_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_set_thinking_cmd<W: tokio::io::AsyncWrite + Unpin>(
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

pub(crate) async fn handle_config_update_cmd<W: tokio::io::AsyncWrite + Unpin>(
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
