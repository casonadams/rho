use dioxus::prelude::*;
use rho_harness_core::rpc::ticket::parse_ticket_info;
use serde::{Deserialize, Serialize};

const STORAGE_KEY: &str = "rho_fleet_nodes_v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: String,
    pub label: String,
    pub ticket: String,
    pub workspace: Option<String>,
    pub branch: Option<String>,
    pub status: String,
    pub last_seen: u64,
}

pub struct FleetStore;

impl FleetStore {
    pub fn load_nodes() -> Vec<NodeRecord> {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return Vec::new(),
        };
        let storage = match window.local_storage() {
            Ok(Some(s)) => s,
            _ => return Vec::new(),
        };
        match storage.get_item(STORAGE_KEY) {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn save_nodes(nodes: &[NodeRecord]) {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let storage = match window.local_storage() {
            Ok(Some(s)) => s,
            _ => return,
        };
        if let Ok(json) = serde_json::to_string(nodes) {
            let _ = storage.set_item(STORAGE_KEY, &json);
        }
    }

    pub fn add_or_update(node: NodeRecord) {
        let mut nodes = Self::load_nodes();
        if let Some(pos) = nodes.iter().position(|n| n.id == node.id) {
            nodes[pos] = node;
        } else {
            nodes.push(node);
        }
        Self::save_nodes(&nodes);
    }

    pub fn remove(id: &str) {
        let mut nodes = Self::load_nodes();
        nodes.retain(|n| n.id != id);
        Self::save_nodes(&nodes);
    }
}

#[component]
pub fn FleetView(
    nodes: Signal<Vec<NodeRecord>>,
    on_select_node: EventHandler<NodeRecord>,
    on_add_node_clicked: EventHandler<()>,
) -> Element {
    let node_list = nodes.read();

    rsx! {
        section { id: "fleet-view",
            div { class: "fleet-title-row",
                div {
                    h1 { "Active Fleet" }
                    p { class: "fleet-subtitle",
                        "Zero-cloud peer-to-peer control of your rho nodes, powered by "
                        a {
                            class: "iroh-link",
                            href: "https://iroh.computer",
                            target: "_blank",
                            rel: "noopener",
                            "Iroh"
                        }
                    }
                }
            }

            div { class: "node-grid", id: "node-grid",
                if node_list.is_empty() {
                    div {
                        style: "grid-column: 1 / -1; text-align: center; padding: 3rem; color: var(--text-muted);",
                        p { style: "margin-bottom: 1rem;", "No nodes registered in your fleet yet." }
                        p { style: "font-size: 0.85rem;",
                            "Run "
                            code { "rho serve" }
                            " on any machine or click \"+ Add Node\" to pair."
                        }
                    }
                } else {
                    for node in node_list.iter().cloned() {
                        NodeCard {
                            node: node.clone(),
                            on_select: {
                                let node_clone = node.clone();
                                move |_| on_select_node.call(node_clone.clone())
                            },
                            on_remove: {
                                let node_id = node.id.clone();
                                move |_| {
                                    FleetStore::remove(&node_id);
                                    let mut n = nodes.write();
                                    n.retain(|item| item.id != node_id);
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NodeCard(node: NodeRecord, on_select: EventHandler<()>, on_remove: EventHandler<()>) -> Element {
    let ws = node.workspace.as_deref().unwrap_or("~/workspace");
    let br = node.branch.as_deref().unwrap_or("main");
    let short_id = if node.id.len() > 12 { &node.id[..12] } else { &node.id };

    rsx! {
        div {
            class: "node-card",
            onclick: move |_| on_select.call(()),
            div { class: "node-card-header",
                span { class: "node-name", "{node.label}" }
                span { class: "status-badge online",
                    span { class: "status-dot" }
                    " Online"
                }
            }
            div { class: "node-details",
                div { class: "node-workspace", "📁 {ws}" }
                div { class: "node-branch", "🌿 {br}" }
            }
            div { class: "node-card-footer",
                span { "{short_id}" }
                button {
                    class: "btn-secondary",
                    style: "padding: 2px 8px; font-size: 0.75rem;",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_remove.call(());
                    },
                    "Remove"
                }
            }
        }
    }
}

#[component]
pub fn AddNodeModal(on_close: EventHandler<()>, on_paired: EventHandler<NodeRecord>) -> Element {
    let mut ticket_input = use_signal(String::new);
    let mut label_input = use_signal(String::new);
    let mut error_msg = use_signal(|| Option::<String>::None);

    let handle_pair = move |_| {
        let raw = ticket_input.read().trim().to_string();
        let label = label_input.read().trim().to_string();
        if raw.is_empty() {
            return;
        }

        match parse_ticket_info(&raw) {
            Ok(parsed) => {
                let id = parsed.endpoint_id.clone();
                let display_label = if label.is_empty() {
                    let short = if id.len() > 8 { &id[..8] } else { &id };
                    format!("Node {short}")
                } else {
                    label
                };

                let record = NodeRecord {
                    id,
                    label: display_label,
                    ticket: raw,
                    workspace: None,
                    branch: None,
                    status: "online".to_string(),
                    last_seen: js_sys::Date::now() as u64,
                };
                FleetStore::add_or_update(record.clone());
                on_paired.call(record);
            }
            Err(e) => {
                error_msg.set(Some(format!("Invalid node ticket: {e}")));
            }
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
                h2 { "Pair Remote Node" }
                p { style: "font-size: 0.85rem; color: var(--text-secondary);",
                    "Enter the pairing URL or node ticket generated by "
                    code { "rho serve" }
                    " or "
                    code { "/pair" }
                    "."
                }
                input {
                    r#type: "text",
                    id: "node-ticket-input",
                    placeholder: "rho_... or https://...#ticket=rho_...",
                    value: "{ticket_input}",
                    oninput: move |e| ticket_input.set(e.value()),
                }
                input {
                    r#type: "text",
                    id: "node-label-input",
                    placeholder: "Friendly Label (e.g. Work MacBook, Cloud Devbox)",
                    style: "margin-top: 0.5rem;",
                    value: "{label_input}",
                    oninput: move |e| label_input.set(e.value()),
                }
                if let Some(err) = error_msg.read().as_ref() {
                    p { style: "color: var(--accent-red); font-size: 0.8rem; margin-top: 0.5rem;", "{err}" }
                }
                div { class: "modal-footer",
                    button {
                        class: "btn-secondary",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        class: "btn-primary",
                        onclick: handle_pair,
                        "Pair Node"
                    }
                }
            }
        }
    }
}
