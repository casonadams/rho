use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl<'de> Deserialize<'de> for TaskStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let normalized = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "pending" => Ok(Self::Pending),
            "in_progress" | "inprogress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "deleted" => Ok(Self::Deleted),
            other => Err(serde::de::Error::custom(format!(
                "Unknown status '{other}'. Expected pending, in_progress, completed, deleted"
            ))),
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TodoTask {
    pub id: usize,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "blockedBy")]
    pub blocked_by: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoAction {
    Create,
    Update,
    List,
    Get,
    Delete,
    Clear,
}

impl<'de> Deserialize<'de> for TodoAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.trim().to_ascii_lowercase().as_str() {
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "list" => Ok(Self::List),
            "get" => Ok(Self::Get),
            "delete" => Ok(Self::Delete),
            "clear" => Ok(Self::Clear),
            other => Err(serde::de::Error::custom(format!(
                "Unknown action '{other}'. Expected create, update, list, get, delete, clear"
            ))),
        }
    }
}

pub(crate) fn deserialize_optional_id<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u as usize))
            } else if let Some(f) = n.as_f64() {
                Ok(Some(f as usize))
            } else {
                Err(serde::de::Error::custom("id must be a non-negative integer"))
            }
        }
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<usize>()
                    .map(Some)
                    .map_err(|_| serde::de::Error::custom(format!("Invalid integer id '{s}'")))
            }
        }
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Err(serde::de::Error::custom("id must be an integer or numeric string")),
    }
}

pub(crate) fn deserialize_optional_id_vec<'de, D>(deserializer: D) -> Result<Option<Vec<usize>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    serde_json::Value::Number(n) => {
                        if let Some(u) = n.as_u64() {
                            ids.push(u as usize);
                        } else if let Some(f) = n.as_f64() {
                            ids.push(f as usize);
                        }
                    }
                    serde_json::Value::String(s) => {
                        if let Ok(u) = s.trim().parse::<usize>() {
                            ids.push(u);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Some(ids))
        }
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(vec![u as usize]))
            } else {
                Ok(None)
            }
        }
        Some(serde_json::Value::String(s)) => {
            if let Ok(u) = s.trim().parse::<usize>() {
                Ok(Some(vec![u]))
            } else {
                Ok(None)
            }
        }
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Err(serde::de::Error::custom("expected an array of integer task IDs")),
    }
}

pub(crate) fn deserialize_optional_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::Bool(b)) => Ok(Some(b)),
        Some(serde_json::Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(Some(true)),
            "false" | "0" | "no" => Ok(Some(false)),
            _ => Ok(None),
        },
        Some(serde_json::Value::Number(n)) => Ok(Some(n.as_u64().unwrap_or(0) != 0)),
        Some(serde_json::Value::Null) | None => Ok(None),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TodoArgs {
    pub action: TodoAction,
    #[serde(default, deserialize_with = "deserialize_optional_id")]
    pub id: Option<usize>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(default, alias = "active_form", alias = "activeForm", rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(
        default,
        alias = "blocked_by",
        alias = "blockedBy",
        rename = "blockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub blocked_by: Option<Vec<usize>>,
    #[serde(
        default,
        alias = "add_blocked_by",
        alias = "addBlockedBy",
        rename = "addBlockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub add_blocked_by: Option<Vec<usize>>,
    #[serde(
        default,
        alias = "remove_blocked_by",
        alias = "removeBlockedBy",
        rename = "removeBlockedBy",
        deserialize_with = "deserialize_optional_id_vec"
    )]
    pub remove_blocked_by: Option<Vec<usize>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(
        default,
        alias = "include_deleted",
        alias = "includeDeleted",
        rename = "includeDeleted",
        deserialize_with = "deserialize_optional_bool"
    )]
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct TodoCreateParams {
    pub subject: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub blocked_by: Option<Vec<usize>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct TodoUpdateParams {
    pub id: usize,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub add_blocked_by: Option<Vec<usize>>,
    pub remove_blocked_by: Option<Vec<usize>>,
    pub metadata: Option<serde_json::Value>,
}
