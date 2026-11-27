use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, A2uiMessage, A2uiModalRequest, ActionBinding, BoundString, ButtonVariant,
    ChoiceOption, CreateSurface, RowJustify, SurfacePlacement, UpdateComponents, UpdateDataModel,
};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(super) fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

pub(super) fn create_surface(surface_id: &str, root_id: &str, title: Option<&str>) -> A2uiMessage {
    A2uiMessage::CreateSurface(CreateSurface {
        surface_id: surface_id.to_string(),
        catalog_id: None,
        placement: SurfacePlacement::Modal,
        title: title.map(str::to_string),
        priority: 100,
        root_id: root_id.to_string(),
    })
}

pub(super) fn single_select_request(allow_other: bool) -> A2uiModalRequest {
    A2uiModalRequest {
        surface_id: "rho:ask".to_string(),
        title: Some("Database".to_string()),
        messages: vec![
            create_surface("rho:ask", "root", Some("Database")),
            A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: "rho:ask".to_string(),
                components: vec![
                    A2uiComponent::new(
                        "q1",
                        A2uiComponentKind::ChoicePicker {
                            label: Some(BoundString::literal("Pick a database")),
                            options: vec![
                                ChoiceOption {
                                    value: "0".to_string(),
                                    label: BoundString::literal("PostgreSQL"),
                                    description: Some(BoundString::literal("Relational")),
                                },
                                ChoiceOption {
                                    value: "1".to_string(),
                                    label: BoundString::literal("SQLite"),
                                    description: None,
                                },
                            ],
                            selection_path: "/answers/q1".to_string(),
                            multi_select: false,
                            allow_other,
                            other_path: allow_other.then(|| "/answers/q1_other".to_string()),
                        },
                    ),
                    A2uiComponent::new(
                        "cancel_btn",
                        A2uiComponentKind::Button {
                            label: BoundString::literal("Cancel"),
                            action: ActionBinding {
                                name: "cancel".to_string(),
                                context: BTreeMap::new(),
                            },
                            variant: ButtonVariant::Secondary,
                        },
                    ),
                    A2uiComponent::new(
                        "submit_btn",
                        A2uiComponentKind::Button {
                            label: BoundString::literal("Submit"),
                            action: ActionBinding {
                                name: "submit".to_string(),
                                context: BTreeMap::new(),
                            },
                            variant: ButtonVariant::Primary,
                        },
                    ),
                    A2uiComponent::new(
                        "actions",
                        A2uiComponentKind::Row {
                            children: vec!["cancel_btn".to_string(), "submit_btn".to_string()],
                            gap: Some(1),
                            justify: Some(RowJustify::End),
                        },
                    ),
                    A2uiComponent::new(
                        "root",
                        A2uiComponentKind::Column {
                            children: vec!["q1".to_string(), "actions".to_string()],
                            gap: Some(1),
                        },
                    ),
                ],
            }),
            A2uiMessage::UpdateDataModel(UpdateDataModel {
                surface_id: "rho:ask".to_string(),
                path: None,
                value: json!({ "answers": { "q1": null } }),
            }),
        ],
        timeout_ms: None,
    }
}
