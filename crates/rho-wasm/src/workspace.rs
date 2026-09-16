use std::path::Path;

use chrono::DateTime;
use dioxus::prelude::*;
use rho_harness_core::rpc::protocol::RpcCommand;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::footer::abbreviate_home;
use rho_ui_core::ir::{ChangeType, ContentBlock, DiagramKind, InlineSpan};
use rho_ui_core::session::{SessionCommand, SessionState, SessionTurnState};
use rho_ui_core::state::{FooterMetrics, SecretGuard, format_duration_ms, format_relative_time, format_tokens};
use serde::{Deserialize, Serialize};

use crate::fleet::NodeRecord;
use crate::transport::WebClient;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub preview: Option<String>,
    pub last_modified: Option<u64>,
}

#[component]
pub fn WorkspaceView(
    node: NodeRecord,
    client: Signal<Option<WebClient>>,
    session_state: Signal<SessionState>,
    footer_metrics: Signal<FooterMetrics>,
    sessions: Signal<Vec<SessionSummary>>,
    active_session_id: Signal<Option<String>>,
    on_back_to_fleet: EventHandler<()>,
    on_open_auth: EventHandler<()>,
    on_open_settings: EventHandler<()>,
    on_open_export: EventHandler<()>,
) -> Element {
    let mut sidebar_collapsed = use_signal(|| {
        let window = web_sys::window();
        let storage = window.as_ref().and_then(|w| w.local_storage().ok().flatten());
        storage.and_then(|s| s.get_item("rho_sidebar_collapsed").ok().flatten()) == Some("true".to_string())
    });

    let mut prompt_input = use_signal(String::new);
    let mut autocomplete_matches = use_signal(Vec::<String>::new);
    let mut show_autocomplete = use_signal(|| false);

    let is_working = matches!(
        session_state.read().turn_state,
        SessionTurnState::Running { .. } | SessionTurnState::AwaitingApproval
    );

    let active_tool_name = match &session_state.read().turn_state {
        SessionTurnState::Running { active_tool } => active_tool.clone(),
        _ => None,
    };

    let handle_new_session = move |_| {
        let cl = client.read().clone();
        if let Some(c) = cl {
            session_state.write().handle_command(SessionCommand::Clear);
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(resp) = c.send_command(RpcCommand::CreateSession { workspace: None }).await
                    && let Some(data) = resp.data
                    && let Some(sid) = data.get("session_id").and_then(|v| v.as_str())
                {
                    active_session_id.set(Some(sid.to_string()));
                }
                if let Ok(resp) = c.send_command(RpcCommand::ListSessions).await
                    && let Some(data) = resp.data
                    && let Ok(s_list) = serde_json::from_value::<Vec<SessionSummary>>(data)
                {
                    sessions.set(s_list);
                }
            });
        }
    };

    let current_sid = active_session_id.read().clone();
    let display_title = node.label.clone();
    let display_ws = node.workspace.as_deref().unwrap_or("~/workspace");

    rsx! {
        section { id: "workspace-view", style: "display: flex; flex-direction: column; flex: 1; height: 100%; overflow: hidden;",
            div { class: "workspace-topbar",
                div { class: "workspace-meta",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_back_to_fleet.call(()),
                        "← Fleet"
                    }
                    button {
                        class: "btn-secondary",
                        title: "Toggle Sessions Sidebar",
                        onclick: move |_| {
                            let curr = *sidebar_collapsed.read();
                            sidebar_collapsed.set(!curr);
                            if let Some(w) = web_sys::window()
                                && let Ok(Some(s)) = w.local_storage()
                            {
                                let _ = s.set_item("rho_sidebar_collapsed", if !curr { "true" } else { "false" });
                            }
                        },
                        "≡"
                    }
                    strong { "{display_title}" }
                    span { style: "color: var(--text-muted);", "|" }
                    span { style: "color: var(--text-secondary); font-size: 0.8rem;", "{display_ws}" }
                    if is_working {
                        span { class: "working-pill",
                            span { class: "spinner-ring" }
                            if let Some(t) = &active_tool_name {
                                " running {t}..."
                            } else {
                                " working..."
                            }
                        }
                    }
                }
                div { class: "header-actions",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_open_auth.call(()),
                        "Auth"
                    }
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_open_settings.call(()),
                        "Settings"
                    }
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_open_export.call(()),
                        "Export"
                    }
                    button {
                        class: "btn-primary",
                        onclick: handle_new_session,
                        "+ New Session"
                    }
                }
            }

            div { class: "workspace-layout", style: "flex: 1; display: flex; overflow: hidden;",
                aside {
                    class: if *sidebar_collapsed.read() { "session-sidebar collapsed" } else { "session-sidebar" },
                    div { class: "sidebar-header", "Sessions" }
                    ul { class: "session-list",
                        for s in sessions.read().iter().cloned() {
                            SessionListItem {
                                summary: s.clone(),
                                is_active: current_sid.as_deref() == Some(&s.session_id),
                                on_select: {
                                    let sid = s.session_id.clone();
                                    let cl = client.read().clone();
                                    move |_| {
                                        active_session_id.set(Some(sid.clone()));
                                        session_state.write().handle_command(SessionCommand::Clear);
                                        if let Some(c) = cl.clone() {
                                            let sid_clone = sid.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = c.send_command(RpcCommand::ResumeSession { session_id: sid_clone }).await;
                                            });
                                        }
                                    }
                                },
                            }
                        }
                    }
                }

                section { class: "session-chat", style: "flex: 1; display: flex; flex-direction: column; overflow: hidden; position: relative;",
                    ChatTranscript { blocks: session_state.read().blocks.clone() }

                    if *show_autocomplete.read() {
                        div {
                            style: "position: absolute; bottom: 80px; left: 24px; max-height: 180px; overflow-y: auto; background: var(--bg-surface-raised); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); box-shadow: 0 4px 12px rgba(0,0,0,0.5); z-index: 100;",
                            for m in autocomplete_matches.read().iter().cloned() {
                                div {
                                    style: "padding: 6px 12px; font-size: 0.8rem; cursor: pointer; color: var(--text-primary); border-bottom: 1px solid var(--border-subtle);",
                                    onclick: {
                                        let text = m.clone();
                                        move |_| {
                                            prompt_input.set(format!("{text} "));
                                            show_autocomplete.set(false);
                                        }
                                    },
                                    "{m}"
                                }
                            }
                        }
                    }

                    div { class: "chat-input-divider",
                        div { class: "chat-divider-left",
                            if is_working {
                                span { class: "chat-divider-act",
                                    span { class: "spinner-ring" }
                                    span { class: "chat-divider-label",
                                        if let Some(t) = &active_tool_name {
                                            "{t}"
                                        } else {
                                            "working"
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "chat-divider-line" }
                        div { class: "chat-divider-right", "rho" }
                    }

                    div { class: "chat-input-bar",
                        textarea {
                            id: "chat-prompt",
                            rows: "1",
                            placeholder: if is_working {
                                "Send steering instruction to redirect agent... (Enter to steer)"
                            } else {
                                "Send prompt to remote node... (Enter to send, Shift+Enter for newline)"
                            },
                            value: "{prompt_input}",
                            oninput: move |e| {
                                let val = e.value();
                                prompt_input.set(val.clone());
                                if val.starts_with('/') {
                                    let engine = CompletionEngine::default();
                                    let cands = engine.complete(&val, val.len());
                                    let matches: Vec<String> = cands.into_iter().map(|c| c.display).collect();
                                    let has_matches = !matches.is_empty();
                                    autocomplete_matches.set(matches);
                                    show_autocomplete.set(has_matches);
                                } else {
                                    show_autocomplete.set(false);
                                }
                            },
                            onkeydown: move |e| {
                                if e.key() == Key::Enter && !e.modifiers().shift() {
                                    e.prevent_default();
                                    let text = prompt_input.read().trim().to_string();
                                    if !text.is_empty() {
                                        prompt_input.set(String::new());
                                        show_autocomplete.set(false);
                                        let cl = client.read().clone();
                                        if let Some(c) = cl {
                                            if is_working {
                                                session_state.write().handle_command(SessionCommand::Steer { text: text.clone() });
                                                wasm_bindgen_futures::spawn_local(async move {
                                                    let _ = c.send_command(RpcCommand::Steer { message: text }).await;
                                                });
                                            } else {
                                                session_state.write().handle_command(SessionCommand::Prompt { text: text.clone() });
                                                wasm_bindgen_futures::spawn_local(async move {
                                                    let _ = c.send_command(RpcCommand::Prompt {
                                                        message: text,
                                                        images: None,
                                                        streaming_behavior: None,
                                                    }).await;
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        button {
                            class: if is_working { "btn-steer" } else { "btn-primary" },
                            onclick: move |_| {
                                let text = prompt_input.read().trim().to_string();
                                if !text.is_empty() {
                                    prompt_input.set(String::new());
                                    show_autocomplete.set(false);
                                    let cl = client.read().clone();
                                    if let Some(c) = cl {
                                        if is_working {
                                            session_state.write().handle_command(SessionCommand::Steer { text: text.clone() });
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = c.send_command(RpcCommand::Steer { message: text }).await;
                                            });
                                        } else {
                                            session_state.write().handle_command(SessionCommand::Prompt { text: text.clone() });
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let _ = c.send_command(RpcCommand::Prompt {
                                                    message: text,
                                                    images: None,
                                                    streaming_behavior: None,
                                                }).await;
                                            });
                                        }
                                    }
                                }
                            },
                            if is_working { "Steer ↗" } else { "Send" }
                        }
                    }

                    SessionFooter {
                        metrics: footer_metrics.read().clone(),
                        workspace: display_ws.to_string(),
                        branch: node.branch.clone().unwrap_or_else(|| "main".to_string()),
                    }
                }
            }
        }
    }
}

#[component]
fn SessionListItem(summary: SessionSummary, is_active: bool, on_select: EventHandler<()>) -> Element {
    let title = summary
        .name
        .clone()
        .unwrap_or_else(|| summary.preview.clone().unwrap_or_else(|| summary.session_id.clone()));
    let time_str = if let Some(ts) = summary.last_modified {
        DateTime::from_timestamp_millis(ts as i64)
            .map(format_relative_time)
            .unwrap_or_default()
    } else {
        String::new()
    };

    rsx! {
        li {
            class: if is_active { "session-item active" } else { "session-item" },
            onclick: move |_| on_select.call(()),
            div { style: "font-weight: 600; color: var(--text-primary); word-break: break-word;", "{title}" }
            if !time_str.is_empty() {
                div { style: "font-size: 0.7rem; color: var(--text-muted);", "{time_str}" }
            }
        }
    }
}

#[component]
fn ChatTranscript(blocks: Vec<ContentBlock>) -> Element {
    rsx! {
        div { class: "chat-transcript", style: "flex: 1; overflow-y: auto; padding: 1.5rem;",
            for block in blocks {
                BlockItem { block: block.clone() }
            }
        }
    }
}

#[component]
fn BlockItem(block: ContentBlock) -> Element {
    match block {
        ContentBlock::UserPrompt { text, .. } => {
            let redacted = SecretGuard::redact(&text);
            rsx! {
                div { class: "chat-bubble user", "{redacted}" }
            }
        }
        ContentBlock::Thinking {
            content, duration_ms, ..
        } => {
            let mut expanded = use_signal(|| false);
            let d_str = duration_ms.map(format_duration_ms).unwrap_or_default();
            rsx! {
                div { class: "thinking-block",
                    div {
                        class: "thinking-header",
                        onclick: move |_| {
                            let curr = *expanded.read();
                            expanded.set(!curr);
                        },
                        span { "🧠 Thinking " }
                        if !d_str.is_empty() {
                            span { style: "font-size: 0.75rem; color: var(--text-muted); margin-left: 0.5rem;", "({d_str})" }
                        }
                    }
                    if *expanded.read() {
                        div { class: "thinking-content", style: "display: block;", "{content}" }
                    }
                }
            }
        }
        ContentBlock::Paragraph(inlines) => {
            rsx! {
                div { class: "chat-bubble assistant",
                    div { class: "prose-content",
                        p {
                            for span in inlines {
                                InlineSpanItem { span: span.clone() }
                            }
                        }
                    }
                }
            }
        }
        ContentBlock::Heading { level, content } => {
            rsx! {
                div { class: "chat-bubble assistant",
                    div { class: "prose-content",
                        match level {
                            1 => rsx! { h1 { for span in content { InlineSpanItem { span } } } },
                            2 => rsx! { h2 { for span in content { InlineSpanItem { span } } } },
                            _ => rsx! { h3 { for span in content { InlineSpanItem { span } } } },
                        }
                    }
                }
            }
        }
        ContentBlock::CodeFence { language, content, .. } => {
            let mut copied = use_signal(|| false);
            let code_copy = content.clone();
            rsx! {
                div { class: "code-block",
                    div { class: "code-lang",
                        span { "{language}" }
                        button {
                            style: "background: none; border: none; color: var(--text-muted); cursor: pointer; font-size: 0.75rem;",
                            onclick: move |_| {
                                if let Some(w) = web_sys::window() {
                                    let nav = w.navigator();
                                    let clipboard = nav.clipboard();
                                    let _ = clipboard.write_text(&code_copy);
                                    copied.set(true);
                                }
                            },
                            if *copied.read() { "✓ Copied" } else { "Copy" }
                        }
                    }
                    pre { code { "{content}" } }
                }
            }
        }
        ContentBlock::ToolCall(inv) => {
            rsx! {
                div { class: "tool-activity-card running",
                    div { class: "tool-activity-header",
                        div { class: "tool-header-left",
                            span { class: "tool-header-name", "{inv.name}" }
                            span { class: "tool-header-args", "{inv.args_summary}" }
                        }
                        div { class: "tool-header-right",
                            span { class: "spinner-ring" }
                        }
                    }
                }
            }
        }
        ContentBlock::ToolResult {
            invocation,
            output,
            is_error,
            duration_ms,
            ..
        } => {
            let mut expanded = use_signal(|| is_error || invocation.name == "bash" || invocation.name == "edit");
            let d_str = duration_ms.map(format_duration_ms).unwrap_or_default();
            let redacted_out = SecretGuard::redact(&output);

            rsx! {
                div { class: if is_error { "tool-activity-card is-error" } else { "tool-activity-card" },
                    div {
                        class: "tool-activity-header",
                        onclick: move |_| {
                            let curr = *expanded.read();
                            expanded.set(!curr);
                        },
                        div { class: "tool-header-left",
                            span { class: "tool-header-name", "{invocation.name}" }
                            span { class: "tool-header-args", "{invocation.args_summary}" }
                        }
                        div { class: "tool-header-right",
                            if !d_str.is_empty() {
                                span { style: "font-size: 0.75rem; color: var(--text-muted); margin-right: 0.5rem;", "{d_str}" }
                            }
                            span { class: "tool-chevron", if *expanded.read() { "▾" } else { "▸" } }
                        }
                    }
                    if *expanded.read() && !redacted_out.trim().is_empty() {
                        div { class: "tool-activity-body", style: "display: block; white-space: pre-wrap;",
                            "{redacted_out}"
                        }
                    }
                }
            }
        }
        ContentBlock::Diff { hunks, file_path, .. } => {
            let title = file_path.unwrap_or_else(|| "diff".to_string());
            rsx! {
                div { class: "code-block",
                    div { class: "code-lang", "{title}" }
                    pre {
                        for hunk in hunks {
                            match hunk.change {
                                ChangeType::Added => rsx! {
                                    div { style: "color: var(--accent-green);", "+ {hunk.line}" }
                                },
                                ChangeType::Removed => rsx! {
                                    div { style: "color: var(--accent-red);", "- {hunk.line}" }
                                },
                                ChangeType::Equal => rsx! {
                                    div { style: "color: var(--text-secondary);", "  {hunk.line}" }
                                },
                            }
                        }
                    }
                }
            }
        }
        ContentBlock::Table { headers, rows } => {
            rsx! {
                div { style: "overflow-x: auto; margin: 0.5rem 0;",
                    table { style: "border-collapse: collapse; width: 100%; font-size: 0.85rem;",
                        thead {
                            tr { style: "border-bottom: 2px solid var(--border-subtle);",
                                for h in headers {
                                    th { style: "padding: 6px 12px; text-align: left;",
                                        for span in h { InlineSpanItem { span } }
                                    }
                                }
                            }
                        }
                        tbody {
                            for r in rows {
                                tr { style: "border-bottom: 1px solid var(--border-subtle);",
                                    for cell in r {
                                        td { style: "padding: 6px 12px;",
                                            for span in cell { InlineSpanItem { span } }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        ContentBlock::Diagram { kind, content } => {
            rsx! {
                div { style: "background: var(--bg-surface); padding: 1rem; border-radius: var(--radius-md); margin: 0.5rem 0; border: 1px solid var(--border-subtle);",
                    div { style: "font-size: 0.75rem; color: var(--accent-purple); margin-bottom: 0.5rem;",
                        match kind {
                            DiagramKind::Mermaid => "Mermaid Diagram",
                            DiagramKind::Ascii => "ASCII Diagram",
                        }
                    }
                    pre { style: "font-size: 0.8rem; overflow-x: auto;", "{content}" }
                }
            }
        }
        ContentBlock::Notice { text, is_error } => {
            let color = if is_error {
                "var(--accent-red)"
            } else {
                "var(--accent-blue)"
            };
            rsx! {
                div { style: "font-size: 0.8rem; color: {color}; padding: 0.5rem 0; font-style: italic;",
                    "{text}"
                }
            }
        }
    }
}

#[component]
fn InlineSpanItem(span: InlineSpan) -> Element {
    match span {
        InlineSpan::Text(t) => rsx! { "{t}" },
        InlineSpan::Bold(b) => rsx! { strong { "{b}" } },
        InlineSpan::Italic(i) => rsx! { em { "{i}" } },
        InlineSpan::Code(c) => rsx! { code { class: "inline-code", "{c}" } },
        InlineSpan::Strikethrough(s) => rsx! { s { "{s}" } },
        InlineSpan::Link { label, url } => {
            rsx! {
                a {
                    href: "{url}",
                    target: "_blank",
                    rel: "noopener",
                    "{label}"
                }
            }
        }
        InlineSpan::Styled { text, .. } => rsx! { "{text}" },
    }
}

#[component]
fn SessionFooter(metrics: FooterMetrics, workspace: String, branch: String) -> Element {
    let top_ws = abbreviate_home(Path::new(&workspace), None);
    let top_title = if !branch.is_empty() {
        format!("{top_ws} ({branch})")
    } else {
        top_ws
    };

    let in_tok = format_tokens(metrics.input_tokens);
    let out_tok = format_tokens(metrics.output_tokens);
    let mut stats = format!("↑{in_tok} ↓{out_tok}");

    if metrics.cache_read_tokens > 0 {
        stats.push_str(&format!(" R{}", format_tokens(metrics.cache_read_tokens)));
    }
    if metrics.cache_write_tokens > 0 {
        stats.push_str(&format!(" W{}", format_tokens(metrics.cache_write_tokens)));
    }
    if let Some(cost) = metrics.total_cost
        && cost > 0.0
    {
        stats.push_str(&format!(" ${cost:.3}"));
    }
    let pct = metrics.context_percent();
    if pct > 0.0 {
        if metrics.context_window > 0 {
            stats.push_str(&format!(" {pct:.1}%/{}", format_tokens(metrics.context_window as u64)));
        } else {
            stats.push_str(&format!(" {pct:.1}%"));
        }
    }
    if let Some(tps) = metrics.tokens_per_second
        && tps > 0.0
    {
        stats.push_str(&format!(" @{:.0}t/s", tps));
    }

    rsx! {
        div { class: "session-footer", id: "session-footer",
            div { class: "footer-line top-line",
                div { class: "footer-left", "{top_title}" }
                div { class: "footer-right",
                    if let Some(q) = &metrics.quota_summary {
                        span { class: "footer-quota-badge", "{q}" }
                    }
                }
            }
            div { class: "footer-line stats-line",
                div { class: "footer-left", "{stats}" }
                div { class: "footer-right", "rho • active" }
            }
        }
    }
}
