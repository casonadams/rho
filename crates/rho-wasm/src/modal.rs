use dioxus::prelude::*;
use qrcode::QrCode;
use rho_harness_core::rpc::protocol::RpcCommand;
use rho_ui_core::ir::ContentBlock;
use rho_ui_core::session::{AuthMode, PROVIDER_DEFS};
use wasm_bindgen::{JsCast, JsValue};

use crate::transport::WebClient;

#[component]
pub fn AuthModal(client: Option<WebClient>, on_close: EventHandler<()>) -> Element {
    let mut selected_provider = use_signal(|| "anthropic".to_string());
    let mut manual_api_key = use_signal(String::new);
    let mut status_msg = use_signal(|| Option::<String>::None);
    let mut oauth_url = use_signal(|| Option::<String>::None);
    let mut oauth_user_code = use_signal(|| Option::<String>::None);
    let oauth_interaction_id = use_signal(|| Option::<String>::None);
    let mut oauth_secret_input = use_signal(String::new);

    let current_prov = selected_provider.read().clone();
    let is_key_mode = PROVIDER_DEFS
        .iter()
        .find(|p| p.id == current_prov)
        .map(|p| p.auth_mode == AuthMode::ApiKey)
        .unwrap_or(false);

    let client_clone = client.clone();
    let handle_start = move |_| {
        let prov = selected_provider.read().clone();
        let is_key = PROVIDER_DEFS
            .iter()
            .find(|p| p.id == prov)
            .map(|p| p.auth_mode == AuthMode::ApiKey)
            .unwrap_or(false);

        if is_key {
            let key = manual_api_key.read().trim().to_string();
            if key.is_empty() {
                return;
            }
            if let Some(c) = client_clone.clone() {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = c
                        .send_command(RpcCommand::SetApiKey {
                            provider: prov,
                            api_key: key,
                        })
                        .await;
                });
            }
            status_msg.set(Some("API key saved successfully!".to_string()));
        } else {
            status_msg.set(Some("Initiating OAuth login on remote node...".to_string()));
            if let Some(c) = client_clone.clone() {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = c.send_command(RpcCommand::AuthLogin { provider: prov }).await;
                });
            }
        }
    };

    let client_input = client.clone();
    let handle_submit_token = move |_| {
        let token = oauth_secret_input.read().trim().to_string();
        let iid = oauth_interaction_id.read().clone().unwrap_or_default();
        if let Some(c) = client_input.clone() {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = c
                    .send_command(RpcCommand::AuthInput {
                        interaction_id: iid,
                        secret_value: Some(token),
                        selected_option: None,
                    })
                    .await;
            });
            status_msg.set(Some("Submitting token for verification...".to_string()));
        }
    };

    rsx! {
        div {
            class: "modal-overlay",
            role: "dialog",
            aria_modal: "true",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal-card",
                onclick: |e| e.stop_propagation(),
                h2 { "Authenticate Provider" }
                p { style: "font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 1rem;",
                    "Select a model provider to authenticate on this remote rho node."
                }

                select {
                    id: "auth-provider-select",
                    value: "{selected_provider}",
                    onchange: move |e| {
                        selected_provider.set(e.value());
                        oauth_url.set(None);
                        oauth_user_code.set(None);
                        status_msg.set(None);
                    },
                    for p in PROVIDER_DEFS {
                        option {
                            value: "{p.id}",
                            match p.auth_mode {
                                AuthMode::OAuth => "{p.name} (OAuth)",
                                AuthMode::ApiKey => "{p.name} (API Key)",
                                AuthMode::Local => "{p.name} (Local)",
                            }
                        }
                    }
                }

                if is_key_mode {
                    div { style: "margin-top: 1rem; display: flex; flex-direction: column; gap: 0.5rem;",
                        label { style: "font-size: 0.8rem; color: var(--text-secondary);", "API Key:" }
                        input {
                            r#type: "password",
                            placeholder: "Enter API key...",
                            value: "{manual_api_key}",
                            oninput: move |e| manual_api_key.set(e.value()),
                        }
                    }
                }

                if let Some(url) = oauth_url.read().as_ref() {
                    div { style: "margin-top: 1rem;",
                        a {
                            class: "btn-primary",
                            style: "display: block; text-align: center; text-decoration: none;",
                            href: "{url}",
                            target: "_blank",
                            rel: "noopener",
                            "Open Authorization Window ↗"
                        }
                    }
                }

                if let Some(code) = oauth_user_code.read().as_ref() {
                    div { style: "margin-top: 0.75rem; font-size: 0.85rem;",
                        "Enter confirmation code: "
                        strong { style: "color: var(--accent-green);", "{code}" }
                    }
                }

                if oauth_interaction_id.read().is_some() {
                    div { style: "margin-top: 1rem; display: flex; flex-direction: column; gap: 0.5rem;",
                        label { style: "font-size: 0.8rem; color: var(--text-secondary);", "Authorization Token / Code:" }
                        input {
                            r#type: "password",
                            placeholder: "Paste code or token here...",
                            value: "{oauth_secret_input}",
                            oninput: move |e| oauth_secret_input.set(e.value()),
                        }
                        button {
                            class: "btn-primary",
                            onclick: handle_submit_token,
                            "Submit Code"
                        }
                    }
                }

                if let Some(msg) = status_msg.read().as_ref() {
                    p { style: "font-size: 0.8rem; color: var(--accent-blue); margin-top: 0.75rem;", "{msg}" }
                }

                div { class: "modal-footer",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                    if !is_key_mode && oauth_interaction_id.read().is_none() {
                        button {
                            class: "btn-primary",
                            onclick: handle_start,
                            "Start Login"
                        }
                    } else if is_key_mode {
                        button {
                            class: "btn-primary",
                            onclick: handle_start,
                            "Save Key"
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn SettingsModal(on_close: EventHandler<()>) -> Element {
    let mut thinking_enabled = use_signal(|| true);
    let mut vim_mode_enabled = use_signal(|| false);

    rsx! {
        div {
            class: "modal-overlay",
            role: "dialog",
            aria_modal: "true",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal-card",
                onclick: |e| e.stop_propagation(),
                h2 { "Settings" }
                div { style: "display: flex; flex-direction: column; gap: 1rem; margin: 1.5rem 0;",
                    label { style: "display: flex; align-items: center; gap: 0.75rem; font-size: 0.85rem; cursor: pointer;",
                        input {
                            r#type: "checkbox",
                            checked: *thinking_enabled.read(),
                            onchange: move |e| thinking_enabled.set(e.checked()),
                        }
                        "Show reasoning & thinking streams"
                    }
                    label { style: "display: flex; align-items: center; gap: 0.75rem; font-size: 0.85rem; cursor: pointer;",
                        input {
                            r#type: "checkbox",
                            checked: *vim_mode_enabled.read(),
                            onchange: move |e| vim_mode_enabled.set(e.checked()),
                        }
                        "Enable Vim navigation keybindings in editor"
                    }
                }
                div { class: "modal-footer",
                    button {
                        class: "btn-primary",
                        onclick: move |_| on_close.call(()),
                        "Done"
                    }
                }
            }
        }
    }
}

#[component]
pub fn PairingQrModal(ticket: String, on_close: EventHandler<()>) -> Element {
    let mut copied = use_signal(|| false);

    let qr_svg = match QrCode::new(ticket.as_bytes()) {
        Ok(code) => code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(200, 200)
            .build(),
        Err(_) => String::new(),
    };

    let ticket_copy = ticket.clone();

    rsx! {
        div {
            class: "modal-overlay",
            role: "dialog",
            aria_modal: "true",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal-card",
                style: "max-width: 480px; text-align: center;",
                onclick: |e| e.stop_propagation(),
                h2 { "Remote Node Pairing" }
                p { style: "font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 1rem;",
                    "Scan with your mobile device or copy the pairing ticket to connect."
                }

                if !qr_svg.is_empty() {
                    div {
                        style: "background: #fff; padding: 1rem; border-radius: var(--radius-md); display: inline-block; margin-bottom: 1rem;",
                        dangerous_inner_html: "{qr_svg}"
                    }
                }

                div { style: "margin-bottom: 1rem;",
                    textarea {
                        readonly: true,
                        style: "width: 100%; height: 60px; font-size: 0.75rem; background: var(--bg-base); color: var(--text-secondary); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); padding: 0.5rem; word-break: break-all;",
                        value: "{ticket}"
                    }
                }

                div { class: "modal-footer", style: "justify-content: center; gap: 1rem;",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                    button {
                        class: "btn-primary",
                        onclick: move |_| {
                            if let Some(w) = web_sys::window() {
                                let _ = w.navigator().clipboard().write_text(&ticket_copy);
                                copied.set(true);
                            }
                        },
                        if *copied.read() { "✓ Copied Ticket" } else { "Copy Ticket" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn ExportModal(blocks: Vec<ContentBlock>, session_id: String, on_close: EventHandler<()>) -> Element {
    let handle_download_markdown = {
        let bl = blocks.clone();
        let sid = session_id.clone();
        move |_| {
            let mut md = format!("# rho session: {sid}\n\n");
            for b in &bl {
                match b {
                    ContentBlock::UserPrompt { text, .. } => {
                        md.push_str(&format!("## User\n\n{text}\n\n"));
                    }
                    ContentBlock::Paragraph(inlines) => {
                        let text = inlines.iter().map(|s| s.plain_text()).collect::<Vec<_>>().join("");
                        md.push_str(&format!("{text}\n\n"));
                    }
                    ContentBlock::CodeFence { language, content, .. } => {
                        md.push_str(&format!("```{language}\n{content}\n```\n\n"));
                    }
                    ContentBlock::ToolResult { invocation, output, .. } => {
                        md.push_str(&format!("### Tool: {}\n```\n{}\n```\n\n", invocation.name, output));
                    }
                    _ => {}
                }
            }
            trigger_browser_download(&format!("{sid}.md"), "text/markdown", &md);
        }
    };

    let handle_download_html = {
        let bl = blocks.clone();
        let sid = session_id.clone();
        move |_| {
            let mut html = format!(
                "<!DOCTYPE html><html><head><title>rho session: {sid}</title><style>body {{ font-family: monospace; padding: 2rem; background: #0a0d13; color: #f2f5fa; }}</style></head><body>"
            );
            html.push_str(&format!("<h1>rho session: {sid}</h1>"));
            for b in &bl {
                match b {
                    ContentBlock::UserPrompt { text, .. } => {
                        html.push_str(&format!("<h2>User</h2><p>{}</p>", html_escape(text)));
                    }
                    ContentBlock::Paragraph(inlines) => {
                        let text = inlines.iter().map(|s| s.plain_text()).collect::<Vec<_>>().join("");
                        html.push_str(&format!("<p>{}</p>", html_escape(&text)));
                    }
                    ContentBlock::CodeFence { language, content, .. } => {
                        html.push_str(&format!(
                            "<pre><code class=\"{}\">{}</code></pre>",
                            language,
                            html_escape(content)
                        ));
                    }
                    ContentBlock::ToolResult { invocation, output, .. } => {
                        html.push_str(&format!(
                            "<h3>Tool: {}</h3><pre>{}</pre>",
                            invocation.name,
                            html_escape(output)
                        ));
                    }
                    _ => {}
                }
            }
            html.push_str("</body></html>");
            trigger_browser_download(&format!("{sid}.html"), "text/html", &html);
        }
    };

    rsx! {
        div {
            class: "modal-overlay",
            role: "dialog",
            aria_modal: "true",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal-card",
                onclick: |e| e.stop_propagation(),
                h2 { "Export Session" }
                p { style: "font-size: 0.85rem; color: var(--text-secondary); margin-bottom: 1.5rem;",
                    "Download this session transcript as a portable Markdown or HTML file."
                }
                div { style: "display: flex; gap: 1rem; justify-content: center; margin-bottom: 1.5rem;",
                    button {
                        class: "btn-primary",
                        onclick: handle_download_markdown,
                        "Download Markdown (.md)"
                    }
                    button {
                        class: "btn-secondary",
                        onclick: handle_download_html,
                        "Download HTML (.html)"
                    }
                }
                div { class: "modal-footer",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_close.call(()),
                        "Close"
                    }
                }
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn trigger_browser_download(filename: &str, mime_type: &str, content: &str) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let document = match window.document() {
        Some(d) => d,
        None => return,
    };

    let blob_parts = js_sys::Array::new();
    blob_parts.push(&JsValue::from_str(content));

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime_type);

    if let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &options)
        && let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob)
        && let Ok(element) = document.create_element("a")
        && let Ok(a) = element.dyn_into::<web_sys::HtmlAnchorElement>()
    {
        a.set_href(&url);
        a.set_download(filename);
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}
