use serde::{Deserialize, Serialize};

pub const AGENT_TASK_ALPN: &[u8] = b"rho/agent/task/v1";
pub const AGENT_EVAL_ALPN: &[u8] = b"rho/agent/eval/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    ReadOnly,
    Propose,
    Full,
}

const CAPABILITY_STRS: [&str; 3] = ["read_only", "propose", "full"];

impl AgentCapability {
    #[must_use]
    pub fn is_tool_allowed(&self, tool: &str) -> bool {
        match self {
            Self::ReadOnly => matches!(tool, "read" | "fd" | "rg" | "lsp" | "web_search" | "web_fetch"),
            Self::Propose => matches!(
                tool,
                "read" | "fd" | "rg" | "lsp" | "web_search" | "web_fetch" | "write" | "edit"
            ),
            Self::Full => true,
        }
    }

    #[must_use]
    pub fn label(&self) -> &'static str {
        CAPABILITY_STRS[*self as usize]
    }
}

impl std::str::FromStr for AgentCapability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let norm = s.trim().to_ascii_lowercase();
        if norm == "read" || norm == "readonly" || norm == "read_only" {
            Ok(Self::ReadOnly)
        } else if norm == "propose" {
            Ok(Self::Propose)
        } else if norm == "full" {
            Ok(Self::Full)
        } else {
            Err(format!("unknown agent capability: '{s}'"))
        }
    }
}

impl std::fmt::Display for AgentCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskRequest {
    pub task_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_slice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentTaskEvent {
    Token {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    Complete {
        result: String,
        turns: usize,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentClientMessage {
    Task(AgentTaskRequest),
    Cancel { task_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentWorkerMessage {
    Event { task_id: String, event: AgentTaskEvent },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_agent_capability_tool_permissions() {
        let ro = AgentCapability::ReadOnly;
        assert!(ro.is_tool_allowed("read"));
        assert!(ro.is_tool_allowed("lsp"));
        assert!(!ro.is_tool_allowed("write"));
        assert!(!ro.is_tool_allowed("bash"));

        let prop = AgentCapability::Propose;
        assert!(prop.is_tool_allowed("read"));
        assert!(prop.is_tool_allowed("write"));
        assert!(prop.is_tool_allowed("edit"));
        assert!(!prop.is_tool_allowed("bash"));

        let full = AgentCapability::Full;
        assert!(full.is_tool_allowed("read"));
        assert!(full.is_tool_allowed("write"));
        assert!(full.is_tool_allowed("bash"));
        assert!(full.is_tool_allowed("any_custom_tool"));
    }

    #[test]
    fn test_agent_capability_parse_and_display() {
        assert_eq!(
            AgentCapability::from_str("readonly").unwrap(),
            AgentCapability::ReadOnly
        );
        assert_eq!(
            AgentCapability::from_str("read_only").unwrap(),
            AgentCapability::ReadOnly
        );
        assert_eq!(AgentCapability::from_str("propose").unwrap(), AgentCapability::Propose);
        assert_eq!(AgentCapability::from_str("full").unwrap(), AgentCapability::Full);
        assert!(AgentCapability::from_str("invalid").is_err());
        assert_eq!(AgentCapability::ReadOnly.to_string(), "read_only");
    }

    #[test]
    fn test_agent_task_event_serde() {
        let ev = AgentTaskEvent::Complete {
            result: "done".into(),
            turns: 3,
        };
        let ser = serde_json::to_string(&ev).unwrap();
        let de: AgentTaskEvent = serde_json::from_str(&ser).unwrap();
        assert_eq!(ev, de);
    }
}
