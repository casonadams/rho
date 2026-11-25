use super::components::A2uiComponent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_CATALOG_ID: &str = "rho.tui.v1";
pub const DEFAULT_ROOT_ID: &str = "root";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SurfacePlacement {
    #[default]
    AboveEditor,
    BelowEditor,
    Modal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSurface {
    pub surface_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub placement: SurfacePlacement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_root_id")]
    pub root_id: String,
}

impl CreateSurface {
    pub fn new(surface_id: impl Into<String>) -> Self {
        Self {
            surface_id: surface_id.into(),
            catalog_id: None,
            placement: SurfacePlacement::AboveEditor,
            title: None,
            priority: 0,
            root_id: default_root_id(),
        }
    }

    pub fn effective_catalog_id(&self) -> &str {
        self.catalog_id.as_deref().unwrap_or(DEFAULT_CATALOG_ID)
    }
}

fn default_root_id() -> String {
    DEFAULT_ROOT_ID.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateComponents {
    pub surface_id: String,
    pub components: Vec<A2uiComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDataModel {
    pub surface_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSurface {
    pub surface_id: String,
}

/// Server-to-client A2UI v1.0 declarative surface message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum A2uiMessage {
    CreateSurface(CreateSurface),
    UpdateComponents(UpdateComponents),
    UpdateDataModel(UpdateDataModel),
    DeleteSurface(DeleteSurface),
}

/// Client-to-server action dispatched when an interactive component is activated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2uiUserAction {
    pub surface_id: String,
    pub action_name: String,
    pub source_component_id: String,
    #[serde(default)]
    pub context: Value,
}

/// Request sent to an interactive `Presenter` to display a modal A2UI surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2uiModalRequest {
    pub surface_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub messages: Vec<A2uiMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Outcome returned from an interactive A2UI modal request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum A2uiModalResult {
    Submitted { action: A2uiUserAction, data_model: Value },
    Cancelled,
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2ui::binding::BoundString;
    use crate::a2ui::components::{A2uiComponentKind, TextUsageHint};
    use serde_json::json;

    #[test]
    fn a2ui_messages_serde_round_trip() {
        let messages = vec![
            A2uiMessage::CreateSurface(CreateSurface {
                surface_id: "rho:demo".to_string(),
                catalog_id: None,
                placement: SurfacePlacement::AboveEditor,
                title: Some("Demo".to_string()),
                priority: 10,
                root_id: "root".to_string(),
            }),
            A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: "rho:demo".to_string(),
                components: vec![A2uiComponent::new(
                    "root",
                    A2uiComponentKind::Text {
                        text: BoundString::path("/msg"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                )],
            }),
            A2uiMessage::UpdateDataModel(UpdateDataModel {
                surface_id: "rho:demo".to_string(),
                path: Some("/msg".to_string()),
                value: json!("hello"),
            }),
            A2uiMessage::DeleteSurface(DeleteSurface {
                surface_id: "rho:demo".to_string(),
            }),
        ];

        for msg in messages {
            let encoded = serde_json::to_string(&msg).unwrap();
            let decoded: A2uiMessage = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn modal_request_and_result_serde_round_trip() {
        let req = A2uiModalRequest {
            surface_id: "ask:1".to_string(),
            title: Some("Clarify".to_string()),
            messages: vec![A2uiMessage::CreateSurface(CreateSurface::new("ask:1"))],
            timeout_ms: Some(30_000),
        };
        let encoded_req = serde_json::to_string(&req).unwrap();
        let decoded_req: A2uiModalRequest = serde_json::from_str(&encoded_req).unwrap();
        assert_eq!(decoded_req, req);

        let res = A2uiModalResult::Submitted {
            action: A2uiUserAction {
                surface_id: "ask:1".to_string(),
                action_name: "submit".to_string(),
                source_component_id: "submit_btn".to_string(),
                context: json!({"step": 1}),
            },
            data_model: json!({"answers": {"q1": "yes"}}),
        };
        let encoded_res = serde_json::to_string(&res).unwrap();
        let decoded_res: A2uiModalResult = serde_json::from_str(&encoded_res).unwrap();
        assert_eq!(decoded_res, res);
    }
}
