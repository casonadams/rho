use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInvocationArgs {
    pub subagent_type: String,
    pub prompt: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub run_in_background: bool,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionResult {
    pub job_id: String,
    pub status: String,
    pub text: String,
    pub tool_calls_count: usize,
    pub is_error: bool,
}
