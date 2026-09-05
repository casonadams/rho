use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    pub surface: String,
    pub pattern: String,
    pub state: PermissionState,
    pub reason: Option<String>,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RuleAction {
    State(PermissionState),
    DenyObject { action: String, reason: Option<String> },
    SurfaceMap(BTreeMap<String, RuleAction>),
}

#[derive(Debug, Default, Deserialize)]
pub struct RawConfigFile {
    #[serde(default)]
    pub permission: BTreeMap<String, RuleAction>,
    #[serde(default)]
    pub allow: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub ask: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub deny: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScopeRules {
    pub rules: Vec<PolicyRule>,
    pub universal: Option<PermissionState>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Policy {
    pub rules: Vec<PolicyRule>,
}

impl Policy {
    pub fn load_with_cwd(cwd: Option<&std::path::Path>) -> (Self, bool) {
        super::io::load_policy(cwd)
    }

    pub fn evaluate(&self, req: crate::permission::EvalRequest<'_>) -> crate::permission::Decision {
        crate::permission::decide_tool_call(self, req)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    First,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDecision {
    pub state: PermissionState,
    pub reason: Option<String>,
    pub matched_pattern: Option<String>,
}
