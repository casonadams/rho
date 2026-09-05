use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
    Ask,
}

#[derive(Debug, Clone, Copy)]
pub struct EvalRequest<'a> {
    pub tool: &'a str,
    pub args: &'a Value,
    pub working_dir: Option<&'a Path>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDraft {
    pub surface: String,
    pub pattern: String,
    pub value: String,
}
