use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMetadata {
    pub name: String,
    pub description: String,
    pub location: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub max_turns: Option<usize>,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOrigin {
    User,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgent {
    pub metadata: AgentMetadata,
    pub origin: AgentOrigin,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentResolutionPaths<'a> {
    pub project_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
}
