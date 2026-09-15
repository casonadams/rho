use dioxus::prelude::*;
use futures::StreamExt;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent};
use rho_harness_core::rpc::ticket::{extract_session_id_from_url, extract_ticket_b64, parse_ticket_info};
use rho_ui_core::ir::{ContentBlock, InlineSpan, ToolInvocation};
use rho_ui_core::session::{SessionCommand, SessionState, SessionTurnState};
use rho_ui_core::state::FooterMetrics;
use wasm_bindgen::JsValue;

use crate::fleet::{AddNodeModal, FleetStore, FleetView, NodeRecord};
use crate::modal::{AuthModal, ExportModal, PairingQrModal, SettingsModal};
use crate::transport::WebClient;
use crate::workspace::{SessionSummary, WorkspaceView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveView {
    Fleet,
    Workspace(NodeRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveModal {
    AddNode,
    Auth,
    Settings,
    PairingQr(String),
    Export,
}

#[derive(Clone, Copy)]
pub struct AppContext {
    pub active_view: Signal<ActiveView>,
    pub client: Signal<Option<WebClient>>,
    pub session_state: Signal<SessionState>,
    pub footer_metrics: Signal<FooterMetrics>,
    pub sessions: Signal<Vec<SessionSummary>>,
    pub active_session_id: Signal<Option<String>>,
}

#[component]
pub fn App() -> Element {
    let mut active_view = use_signal(|| ActiveView::Fleet);
    let mut nodes = use_signal(FleetStore::load_nodes);
    let client = use_signal(|| Option::<WebClient>::None);
    let session_state = use_signal(|| SessionState::new("default", "default"));
    let footer_metrics = use_signal(FooterMetrics::default);
    let sessions = use_signal(Vec::<SessionSummary>::new);
    let active_session_id = use_signal(|| Option::<String>::None);
    let mut active_modal = use_signal(|| Option::<ActiveModal>::None);

    let ctx = AppContext {
        active_view,
        client,
        session_state,
        footer_metrics,
        sessions,
        active_session_id,
    };

    use_hook(move || {
        if let Some(window) = web_sys::window()
            && let Ok(href) = window.location().href()
        {
            let raw_ticket = extract_ticket_b64(&href);
            if !raw_ticket.is_empty()
                && raw_ticket != href.trim()
                && let Ok(parsed) = parse_ticket_info(raw_ticket)
            {
                let short_id = if parsed.endpoint_id.len() > 8 {
                    &parsed.endpoint_id[..8]
                } else {
                    &parsed.endpoint_id
                };
                let record = NodeRecord {
                    id: parsed.endpoint_id.clone(),
                    label: format!("Node {short_id}"),
                    ticket: raw_ticket.to_string(),
                    workspace: None,
                    branch: None,
                    status: "online".to_string(),
                    last_seen: js_sys::Date::now() as u64,
                };
                FleetStore::add_or_update(record.clone());
                nodes.set(FleetStore::load_nodes());

                if let Ok(history) = window.history()
                    && let Ok(pathname) = window.location().pathname()
                {
                    let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&pathname));
                }

                let pref_session = extract_session_id_from_url(&href);
                connect_to_node(record, pref_session, ctx);
            }
        }
    });

    let current_view = active_view.read().clone();

    rsx! {
        header { class: "hub-header",
            a {
                class: "brand",
                href: "#",
                onclick: move |e| {
                    e.prevent_default();
                    active_view.set(ActiveView::Fleet);
                },
                span { "ρ rho" }
                span { class: "brand-badge", "fleet hub" }
            }
            div { class: "header-actions",
                a {
                    class: "powered-by-badge",
                    href: "https://iroh.computer",
                    target: "_blank",
                    rel: "noopener",
                    title: "Powered by Iroh peer-to-peer transport",
                    span { "powered by" }
                    span { class: "iroh-text", "iroh" }
                }
                button {
                    class: "btn-primary",
                    id: "add-node-btn",
                    onclick: move |_| active_modal.set(Some(ActiveModal::AddNode)),
                    "+ Add Node"
                }
            }
        }

        main { class: "hub-main", style: "flex: 1; display: flex; flex-direction: column; overflow: hidden;",
            match current_view {
                ActiveView::Fleet => rsx! {
                    FleetView {
                        nodes,
                        on_select_node: move |node: NodeRecord| {
                            connect_to_node(node, None, ctx);
                        },
                        on_add_node_clicked: move |_| active_modal.set(Some(ActiveModal::AddNode)),
                    }
                },
                ActiveView::Workspace(node) => rsx! {
                    WorkspaceView {
                        node: node.clone(),
                        client,
                        session_state,
                        footer_metrics,
                        sessions,
                        active_session_id,
                        on_back_to_fleet: move |_| active_view.set(ActiveView::Fleet),
                        on_open_auth: move |_| active_modal.set(Some(ActiveModal::Auth)),
                        on_open_settings: move |_| active_modal.set(Some(ActiveModal::Settings)),
                        on_open_export: move |_| active_modal.set(Some(ActiveModal::Export)),
                    }
                },
            }
        }

        if let Some(modal) = active_modal.read().clone() {
            match modal {
                ActiveModal::AddNode => rsx! {
                    AddNodeModal {
                        on_close: move |_| active_modal.set(None),
                        on_paired: move |node: NodeRecord| {
                            nodes.set(FleetStore::load_nodes());
                            active_modal.set(None);
                            connect_to_node(node, None, ctx);
                        },
                    }
                },
                ActiveModal::Auth => rsx! {
                    AuthModal {
                        client: client.read().clone(),
                        on_close: move |_| active_modal.set(None),
                    }
                },
                ActiveModal::Settings => rsx! {
                    SettingsModal {
                        on_close: move |_| active_modal.set(None),
                    }
                },
                ActiveModal::PairingQr(ticket) => rsx! {
                    PairingQrModal {
                        ticket,
                        on_close: move |_| active_modal.set(None),
                    }
                },
                ActiveModal::Export => rsx! {
                    ExportModal {
                        blocks: session_state.read().blocks.clone(),
                        session_id: active_session_id.read().clone().unwrap_or_else(|| "session".to_string()),
                        on_close: move |_| active_modal.set(None),
                    }
                },
            }
        }
    }
}

fn connect_to_node(node: NodeRecord, preferred_session_id: Option<String>, mut ctx: AppContext) {
    ctx.active_view.set(ActiveView::Workspace(node.clone()));
    ctx.session_state.write().handle_command(SessionCommand::Clear);

    let (event_tx, mut event_rx) = futures::channel::mpsc::unbounded::<RpcEvent>();

    wasm_bindgen_futures::spawn_local(async move {
        match WebClient::connect(&node.ticket, event_tx).await {
            Ok(cl) => {
                ctx.client.set(Some(cl.clone()));

                // Start event listener
                let cl_listener = cl.clone();
                let session_state_clone = ctx.session_state;
                let footer_metrics_clone = ctx.footer_metrics;
                wasm_bindgen_futures::spawn_local(async move {
                    while let Some(ev) = event_rx.next().await {
                        handle_rpc_event(ev, session_state_clone, footer_metrics_clone);
                    }
                });

                if let Ok(resp) = cl_listener.send_command(RpcCommand::GetNodeInfo).await
                    && let Some(data) = resp.data
                {
                    let mut updated = node.clone();
                    if let Some(ws) = data.get("active_workspace").and_then(|v| v.as_str()) {
                        updated.workspace = Some(ws.to_string());
                    }
                    if let Some(br) = data.get("active_branch").and_then(|v| v.as_str()) {
                        updated.branch = Some(br.to_string());
                    }
                    FleetStore::add_or_update(updated.clone());
                    ctx.active_view.set(ActiveView::Workspace(updated));
                }

                if let Ok(resp) = cl_listener.send_command(RpcCommand::GetState).await
                    && let Some(data) = resp.data
                {
                    if let Some(sid) = data.get("session_id").and_then(|v| v.as_str()) {
                        ctx.active_session_id.set(Some(sid.to_string()));
                    }
                    update_metrics_from_json(&data, &mut ctx.footer_metrics);
                }

                if let Some(pref_sid) = preferred_session_id {
                    let _ = cl_listener
                        .send_command(RpcCommand::ResumeSession {
                            session_id: pref_sid.clone(),
                        })
                        .await;
                    ctx.active_session_id.set(Some(pref_sid));
                }

                if let Ok(resp) = cl_listener.send_command(RpcCommand::ListSessions).await
                    && let Some(data) = resp.data
                    && let Ok(list) = serde_json::from_value::<Vec<SessionSummary>>(data)
                {
                    ctx.sessions.set(list);
                }
            }
            Err(e) => {
                web_sys::console::error_1(&format!("Failed to connect to node: {e}").into());
                ctx.session_state.write().blocks.push(ContentBlock::Notice {
                    text: format!("Connection failed: {e}"),
                    is_error: true,
                });
            }
        }
    });
}

fn handle_rpc_event(ev: RpcEvent, mut session_state: Signal<SessionState>, mut footer_metrics: Signal<FooterMetrics>) {
    match ev {
        RpcEvent::TurnStart { prompt, .. } => {
            let mut state = session_state.write();
            if !prompt.is_empty() {
                state.blocks.push(ContentBlock::UserPrompt {
                    text: prompt,
                    images: Vec::new(),
                });
            }
            state.turn_state = SessionTurnState::Running { active_tool: None };
        }
        RpcEvent::TextChunk { content } => {
            let mut state = session_state.write();
            if let Some(ContentBlock::Paragraph(inlines)) = state.blocks.last_mut() {
                inlines.push(InlineSpan::Text(content));
            } else {
                state
                    .blocks
                    .push(ContentBlock::Paragraph(vec![InlineSpan::Text(content)]));
            }
        }
        RpcEvent::ReasoningChunk { content } => {
            let mut state = session_state.write();
            if let Some(ContentBlock::Thinking { content: c, .. }) = state.blocks.last_mut() {
                c.push_str(&content);
            } else {
                state.blocks.push(ContentBlock::Thinking {
                    content,
                    is_complete: false,
                    duration_ms: None,
                });
            }
        }
        RpcEvent::ToolCallStart {
            call_id,
            tool,
            arguments,
        } => {
            let mut state = session_state.write();
            state.turn_state = SessionTurnState::Running {
                active_tool: Some(tool.clone()),
            };
            let summary = arguments.to_string();
            state.blocks.push(ContentBlock::ToolCall(ToolInvocation {
                id: call_id,
                name: tool,
                args_summary: if summary.len() > 60 {
                    format!("{}...", &summary[..57])
                } else {
                    summary
                },
                raw_args: Some(arguments.to_string()),
            }));
        }
        RpcEvent::ToolCallResult {
            call_id,
            tool,
            output,
            is_error,
            duration_ms,
        } => {
            let mut state = session_state.write();
            state.turn_state = SessionTurnState::Running { active_tool: None };
            let inv = ToolInvocation {
                id: call_id,
                name: tool,
                args_summary: String::new(),
                raw_args: None,
            };
            state.blocks.push(ContentBlock::ToolResult {
                invocation: inv,
                output,
                is_error,
                duration_ms: Some(duration_ms),
                images: Vec::new(),
            });
        }
        RpcEvent::TurnEnd { .. } => {
            let mut state = session_state.write();
            state.turn_state = SessionTurnState::Idle;
        }
        RpcEvent::StatusChanged { status } => {
            let mut state = session_state.write();
            if status == "busy" {
                state.turn_state = SessionTurnState::Running { active_tool: None };
            } else if status == "waiting_approval" {
                state.turn_state = SessionTurnState::AwaitingApproval;
            } else {
                state.turn_state = SessionTurnState::Idle;
            }
        }
        RpcEvent::Notice { message, .. } => {
            session_state.write().blocks.push(ContentBlock::Notice {
                text: message,
                is_error: false,
            });
        }
        RpcEvent::UsageUpdate {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            total_cost,
            context_percent,
            context_window,
            tokens_per_second,
            quota,
            ..
        } => {
            let mut fm = footer_metrics.write();
            if let Some(i) = input_tokens {
                fm.input_tokens = i;
            }
            if let Some(o) = output_tokens {
                fm.output_tokens = o;
            }
            if let Some(cr) = cache_read_tokens {
                fm.cache_read_tokens = cr;
            }
            if let Some(cw) = cache_write_tokens {
                fm.cache_write_tokens = cw;
            }
            if let Some(tc) = total_cost {
                fm.total_cost = Some(tc);
            }
            if let Some(cw_size) = context_window {
                fm.context_window = cw_size;
            }
            if let Some(cp) = context_percent
                && fm.context_window > 0
            {
                fm.context_tokens = ((cp / 100.0) * fm.context_window as f64) as usize;
            }
            if let Some(tps) = tokens_per_second {
                fm.tokens_per_second = Some(tps);
            }
            if let Some(q) = quota {
                fm.quota_summary = Some(q);
            }
        }
        _ => {}
    }
}

fn update_metrics_from_json(data: &serde_json::Value, footer_metrics: &mut Signal<FooterMetrics>) {
    let mut fm = footer_metrics.write();
    if let Some(v) = data.get("total_input_tokens").and_then(|v| v.as_u64()) {
        fm.input_tokens = v;
    }
    if let Some(v) = data.get("total_output_tokens").and_then(|v| v.as_u64()) {
        fm.output_tokens = v;
    }
    if let Some(v) = data.get("total_cache_read_tokens").and_then(|v| v.as_u64()) {
        fm.cache_read_tokens = v;
    }
    if let Some(v) = data.get("total_cache_write_tokens").and_then(|v| v.as_u64()) {
        fm.cache_write_tokens = v;
    }
    if let Some(v) = data.get("total_cost").and_then(|v| v.as_f64()) {
        fm.total_cost = Some(v);
    }
    if let Some(v) = data.get("context_window").and_then(|v| v.as_u64()) {
        fm.context_window = v as usize;
    }
    if let Some(v) = data.get("quota").and_then(|v| v.as_str()) {
        fm.quota_summary = Some(v.to_string());
    }
}
