//! The Agent2Agent (A2A) v1.0 task data model, used for parent/subagent exchanges.
//! JSON follows the spec's ProtoJSON rules: camelCase fields and SCREAMING_SNAKE_CASE enum values.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
}

impl TaskState {
    /// Terminal tasks cannot be restarted; follow-ups start a new task in the same context.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled | Self::Rejected)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Working => "working",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::InputRequired => "input-required",
            Self::Rejected => "rejected",
            Self::AuthRequired => "auth-required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Part {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            data: None,
        }
    }

    #[must_use]
    pub fn data(data: Value) -> Self {
        Self {
            text: None,
            data: Some(data),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_task_ids: Vec<String>,
}

impl Message {
    #[must_use]
    pub fn text(role: Role, task: &Task, text: impl Into<String>) -> Self {
        Self {
            message_id: uuid::Uuid::new_v4().to_string(),
            context_id: Some(task.context_id.clone()),
            task_id: Some(task.id.clone()),
            role,
            parts: vec![Part::text(text)],
            reference_task_ids: Vec::new(),
        }
    }

    /// The concatenated text parts.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl Task {
    #[must_use]
    pub fn new(id: impl Into<String>, context_id: impl Into<String>, metadata: Option<Value>) -> Self {
        Self {
            id: id.into(),
            context_id: context_id.into(),
            status: TaskStatus {
                state: TaskState::Submitted,
                message: None,
                timestamp: Some(Utc::now()),
            },
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata,
        }
    }

    pub fn set_status(&mut self, state: TaskState, message: Option<Message>) {
        self.status = TaskStatus {
            state,
            message,
            timestamp: Some(Utc::now()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use test_case::test_case;

    #[test]
    fn tasks_serialize_with_the_spec_field_names_and_enum_values() {
        let mut task = Task::new("task-1", "ctx-1", None);
        let mut question = Message::text(Role::Agent, &task, "Which module?");
        question.message_id = "msg-2".into();
        task.set_status(TaskState::InputRequired, Some(question));
        task.status.timestamp = None;
        let mut request = Message::text(Role::User, &task, "Audit performance");
        request.message_id = "msg-1".into();
        request.reference_task_ids = vec!["task-0".into()];
        task.history.push(request);

        let encoded = serde_json::to_value(&task).unwrap();

        assert_eq!(
            encoded,
            json!({
                "id": "task-1",
                "contextId": "ctx-1",
                "status": {
                    "state": "TASK_STATE_INPUT_REQUIRED",
                    "message": {
                        "messageId": "msg-2",
                        "contextId": "ctx-1",
                        "taskId": "task-1",
                        "role": "ROLE_AGENT",
                        "parts": [{ "text": "Which module?" }]
                    }
                },
                "history": [{
                    "messageId": "msg-1",
                    "contextId": "ctx-1",
                    "taskId": "task-1",
                    "role": "ROLE_USER",
                    "parts": [{ "text": "Audit performance" }],
                    "referenceTaskIds": ["task-0"]
                }]
            })
        );
        assert_eq!(serde_json::from_value::<Task>(encoded).unwrap(), task);
    }

    #[test_case(TaskState::Submitted, false; "submitted")]
    #[test_case(TaskState::Working, false; "working")]
    #[test_case(TaskState::InputRequired, false; "input required")]
    #[test_case(TaskState::AuthRequired, false; "auth required")]
    #[test_case(TaskState::Completed, true; "completed")]
    #[test_case(TaskState::Failed, true; "failed")]
    #[test_case(TaskState::Canceled, true; "canceled")]
    #[test_case(TaskState::Rejected, true; "rejected")]
    fn only_completed_failed_canceled_and_rejected_are_terminal(state: TaskState, terminal: bool) {
        assert_eq!(state.is_terminal(), terminal);
    }
}
