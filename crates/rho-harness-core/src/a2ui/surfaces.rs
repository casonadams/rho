use super::binding::BoundString;
use super::components::{A2uiComponent, A2uiComponentKind, RowJustify, SemanticTone, TextUsageHint};
use super::protocol::{A2uiMessage, CreateSurface, DeleteSurface, SurfacePlacement, UpdateComponents, UpdateDataModel};
use crate::todo::{TodoItem, TodoStatus, done_count};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const TODO_SURFACE_ID: &str = "rho:todo";
pub const SUBAGENT_SURFACE_ID: &str = "rho:subagents";
const MAX_VISIBLE_TODO_ITEMS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSurfaceRow {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: String,
    #[serde(default)]
    pub tone: SemanticTone,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

/// Builds A2UI messages for the persistent `rho:todo` surface.
///
/// Emits `DeleteSurface` when `items` is empty or all items are completed.
pub fn todo_surface_messages(items: &[TodoItem]) -> Vec<A2uiMessage> {
    if items.is_empty() || items.iter().all(|item| item.status == TodoStatus::Completed) {
        return vec![A2uiMessage::DeleteSurface(DeleteSurface {
            surface_id: TODO_SURFACE_ID.to_string(),
        })];
    }

    let done = done_count(items);
    let total = items.len();
    let badge_label = format!("{done}/{total}");

    let mut components = Vec::new();
    let mut column_children = Vec::new();

    for (idx, item) in visible_todo_items(items).into_iter().enumerate() {
        let row_id = format!("todo_item_{idx}");
        let (tone, usage_hint) = todo_item_style(item.status);
        components.push(A2uiComponent::new(
            row_id.clone(),
            A2uiComponentKind::Text {
                text: BoundString::literal(format!("{} {}", item.status.marker(), item.content.trim())),
                usage_hint,
                tone,
            },
        ));
        column_children.push(row_id);
    }

    components.push(A2uiComponent::new(
        "todo_badge",
        A2uiComponentKind::Badge {
            label: BoundString::path("/progress"),
            tone: SemanticTone::Accent,
        },
    ));
    components.push(A2uiComponent::new(
        "todo_header",
        A2uiComponentKind::Row {
            children: vec!["todo_title".to_string(), "todo_badge".to_string()],
            gap: Some(1),
            justify: Some(RowJustify::SpaceBetween),
        },
    ));
    components.push(A2uiComponent::new(
        "todo_title",
        A2uiComponentKind::Text {
            text: BoundString::literal("Tasks"),
            usage_hint: TextUsageHint::H3,
            tone: Some(SemanticTone::Neutral),
        },
    ));

    let mut root_children = vec!["todo_header".to_string()];
    root_children.extend(column_children);
    components.push(A2uiComponent::new(
        "todo_body",
        A2uiComponentKind::Column {
            children: root_children,
            gap: Some(0),
        },
    ));
    components.push(A2uiComponent::new(
        "root",
        A2uiComponentKind::Card {
            title: None,
            child: "todo_body".to_string(),
            tone: Some(SemanticTone::Neutral),
        },
    ));

    vec![
        A2uiMessage::CreateSurface(CreateSurface {
            surface_id: TODO_SURFACE_ID.to_string(),
            catalog_id: None,
            placement: SurfacePlacement::AboveEditor,
            title: Some("Tasks".to_string()),
            priority: 10,
            root_id: "root".to_string(),
        }),
        A2uiMessage::UpdateComponents(UpdateComponents {
            surface_id: TODO_SURFACE_ID.to_string(),
            components,
        }),
        A2uiMessage::UpdateDataModel(UpdateDataModel {
            surface_id: TODO_SURFACE_ID.to_string(),
            path: None,
            value: json!({
                "progress": badge_label,
                "done": done,
                "total": total,
            }),
        }),
    ]
}

fn visible_todo_items(items: &[TodoItem]) -> Vec<&TodoItem> {
    let mut visible = Vec::with_capacity(MAX_VISIBLE_TODO_ITEMS);
    for item in items {
        if item.status == TodoStatus::InProgress && visible.len() < MAX_VISIBLE_TODO_ITEMS {
            visible.push(item);
        }
    }
    for item in items {
        if item.status == TodoStatus::Pending && visible.len() < MAX_VISIBLE_TODO_ITEMS {
            visible.push(item);
        }
    }
    visible
}

fn todo_item_style(status: TodoStatus) -> (Option<SemanticTone>, TextUsageHint) {
    match status {
        TodoStatus::InProgress => (Some(SemanticTone::Accent), TextUsageHint::Body),
        TodoStatus::Pending => (None, TextUsageHint::Muted),
        TodoStatus::Completed => (Some(SemanticTone::Success), TextUsageHint::Muted),
    }
}

/// Builds A2UI messages for the persistent `rho:subagents` surface.
///
/// Emits `DeleteSurface` when `rows` is empty.
pub fn subagent_surface_messages(rows: &[SubagentSurfaceRow]) -> Vec<A2uiMessage> {
    if rows.is_empty() {
        return vec![A2uiMessage::DeleteSurface(DeleteSurface {
            surface_id: SUBAGENT_SURFACE_ID.to_string(),
        })];
    }

    let mut components = Vec::new();
    let mut row_ids = Vec::with_capacity(rows.len());

    for (idx, row) in rows.iter().enumerate() {
        let row_id = format!("subagent_row_{idx}");
        let name_id = format!("subagent_name_{idx}");
        let badge_id = format!("subagent_badge_{idx}");

        let label = format_subagent_label(row);
        components.push(A2uiComponent::new(
            name_id.clone(),
            A2uiComponentKind::Text {
                text: BoundString::literal(label),
                usage_hint: TextUsageHint::Body,
                tone: None,
            },
        ));
        components.push(A2uiComponent::new(
            badge_id.clone(),
            A2uiComponentKind::Badge {
                label: BoundString::literal(row.status.clone()),
                tone: row.tone,
            },
        ));

        let mut children = vec![name_id, badge_id];
        if let Some(elapsed_ms) = row.elapsed_ms {
            let elapsed_id = format!("subagent_elapsed_{idx}");
            components.push(A2uiComponent::new(
                elapsed_id.clone(),
                A2uiComponentKind::Text {
                    text: BoundString::literal(format_elapsed(elapsed_ms)),
                    usage_hint: TextUsageHint::Muted,
                    tone: None,
                },
            ));
            children.push(elapsed_id);
        }

        components.push(A2uiComponent::new(
            row_id.clone(),
            A2uiComponentKind::Row {
                children,
                gap: Some(1),
                justify: Some(RowJustify::Start),
            },
        ));
        row_ids.push(row_id);
    }

    components.push(A2uiComponent::new(
        "subagents_body",
        A2uiComponentKind::Column {
            children: row_ids,
            gap: Some(0),
        },
    ));
    components.push(A2uiComponent::new(
        "root",
        A2uiComponentKind::Card {
            title: Some(BoundString::literal("Subagents")),
            child: "subagents_body".to_string(),
            tone: Some(SemanticTone::Neutral),
        },
    ));

    vec![
        A2uiMessage::CreateSurface(CreateSurface {
            surface_id: SUBAGENT_SURFACE_ID.to_string(),
            catalog_id: None,
            placement: SurfacePlacement::AboveEditor,
            title: Some("Subagents".to_string()),
            priority: 20,
            root_id: "root".to_string(),
        }),
        A2uiMessage::UpdateComponents(UpdateComponents {
            surface_id: SUBAGENT_SURFACE_ID.to_string(),
            components,
        }),
    ]
}

fn format_subagent_label(row: &SubagentSurfaceRow) -> String {
    match row.role.as_deref() {
        Some(role) if !role.is_empty() => format!("{} ({role})", row.name),
        _ => row.name.clone(),
    }
}

fn format_elapsed(elapsed_ms: u64) -> String {
    if elapsed_ms < 1_000 {
        format!("{elapsed_ms}ms")
    } else {
        format!("{:.1}s", elapsed_ms as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2ui::store::SurfaceStore;

    #[test]
    fn todo_surface_deletes_when_empty_or_all_completed() {
        assert_eq!(
            todo_surface_messages(&[]),
            vec![A2uiMessage::DeleteSurface(DeleteSurface {
                surface_id: TODO_SURFACE_ID.to_string(),
            })]
        );

        let completed = vec![TodoItem {
            content: "Done".to_string(),
            status: TodoStatus::Completed,
        }];
        assert_eq!(
            todo_surface_messages(&completed),
            vec![A2uiMessage::DeleteSurface(DeleteSurface {
                surface_id: TODO_SURFACE_ID.to_string(),
            })]
        );
    }

    #[test]
    fn todo_surface_builds_valid_above_editor_surface_with_priority_10() {
        let items = vec![
            TodoItem {
                content: "Step 1".to_string(),
                status: TodoStatus::Completed,
            },
            TodoItem {
                content: "Step 2".to_string(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                content: "Step 3".to_string(),
                status: TodoStatus::Pending,
            },
        ];

        let mut store = SurfaceStore::new();
        store.apply_all(todo_surface_messages(&items)).unwrap();

        let surface = store.surface(TODO_SURFACE_ID).unwrap();
        assert_eq!(surface.placement, SurfacePlacement::AboveEditor);
        assert_eq!(surface.priority, 10);
        assert_eq!(surface.resolve_string(&BoundString::path("/progress")), "1/3");
        assert!(surface.component("todo_item_0").is_some());
    }

    #[test]
    fn subagent_surface_deletes_when_empty_and_builds_with_priority_20() {
        assert_eq!(
            subagent_surface_messages(&[]),
            vec![A2uiMessage::DeleteSurface(DeleteSurface {
                surface_id: SUBAGENT_SURFACE_ID.to_string(),
            })]
        );

        let rows = vec![SubagentSurfaceRow {
            id: "t1".to_string(),
            name: "review-pr".to_string(),
            role: Some("review".to_string()),
            status: "running".to_string(),
            tone: SemanticTone::Accent,
            elapsed_ms: Some(1_500),
        }];

        let mut store = SurfaceStore::new();
        store.apply_all(subagent_surface_messages(&rows)).unwrap();

        let surface = store.surface(SUBAGENT_SURFACE_ID).unwrap();
        assert_eq!(surface.placement, SurfacePlacement::AboveEditor);
        assert_eq!(surface.priority, 20);
        assert!(surface.component("subagent_row_0").is_some());
    }
}
