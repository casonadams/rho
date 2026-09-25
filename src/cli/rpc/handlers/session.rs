use super::super::types::RpcDaemonContext;
use crate::error::Result;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcResponse};

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

async fn handle_switch_branch_cmd<W: tokio::io::AsyncWrite + Unpin>(
    node_id: &str,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let eng = ctx.engine.read().await;
    match eng.session_manager.switch_branch(Some(node_id.to_string())).await {
        Ok(_) => {
            ctx.writer
                .write_message(&RpcResponse::success(req_id, "switch_branch", None))
                .await
        }
        Err(e) => {
            ctx.writer
                .write_message(&RpcResponse::failure(req_id, "switch_branch", &e.to_string()))
                .await
        }
    }
}

async fn handle_set_node_label_cmd<W: tokio::io::AsyncWrite + Unpin>(
    node_id: &str,
    label: Option<String>,
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let eng = ctx.engine.read().await;
    let _ = eng.session_manager.set_node_label(node_id, label).await;
    ctx.writer
        .write_message(&RpcResponse::success(req_id, "set_node_label", None))
        .await
}

async fn handle_list_sessions_cmd<W: tokio::io::AsyncWrite + Unpin>(
    req_id: Option<String>,
    ctx: &mut RpcDaemonContext<'_, W>,
) -> Result<()> {
    let cfg = ctx.config.read().await;
    let summaries = rho_harness_core::session::list_session_summaries(&cfg.sessions_dir).unwrap_or_default();
    ctx.writer
        .write_message(&RpcResponse::success(
            req_id,
            "list_sessions",
            Some(serde_json::to_value(summaries).unwrap_or_default()),
        ))
        .await
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
            handle_switch_branch_cmd(node_id, req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::SetNodeLabel { node_id, label } => {
            handle_set_node_label_cmd(node_id, label.clone(), req_id, ctx).await?;
            Ok(true)
        }
        RpcCommand::ListSessions => {
            handle_list_sessions_cmd(req_id, ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn build_engine_session_payload(
    eng: &crate::engine::AgentEngine,
    messages: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let totals = eng.session_usage_totals();
    let base_dir = eng.base_dir.display().to_string();
    let active_branch = crate::ui::interactive::footer::path::get_git_branch(&eng.base_dir);
    let mut payload = serde_json::json!({
        "session_id": eng.session_manager.session_id,
        "workspace": base_dir,
        "active_workspace": base_dir,
        "active_branch": active_branch,
        "model": eng.config.model,
        "provider": eng.config.provider,
        "thinking_level": eng.config.thinking_level,
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
    if let Some(msgs) = messages {
        payload["messages"] = serde_json::Value::Array(msgs);
    }
    payload
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
            let raw_msgs = new_eng.session_manager.load_messages().await.unwrap_or_default();
            let messages = extract_chat_messages(&raw_msgs);
            new_eng.spawn_refresh_quota();
            let payload = build_engine_session_payload(&new_eng, Some(messages));
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
            new_eng.spawn_refresh_quota();
            let payload = build_engine_session_payload(&new_eng, None);
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
