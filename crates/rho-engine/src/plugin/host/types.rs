use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiConfirmParams {
    pub title: String,
    pub message: String,
    #[serde(default = "default_true")]
    pub default_yes: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiConfirmResult {
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSelectOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiSelectParams {
    pub title: String,
    #[serde(default)]
    pub message: String,
    pub options: Vec<HostSelectOption>,
    #[serde(default)]
    pub initial_selection: usize,
    #[serde(default)]
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiSelectResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiInputParams {
    pub title: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiInputResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUiNotifyParams {
    pub message: String,
    #[serde(default = "default_info_level")]
    pub level: String,
}

fn default_info_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSessionAppendParams {
    #[serde(default = "default_custom_kind")]
    pub kind: String,
    pub payload: Value,
}

fn default_custom_kind() -> String {
    "custom".to_string()
}
