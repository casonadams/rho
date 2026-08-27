//! Public data types used across the `ui::render` module.

use crate::tools::bash_ast::RiskTier;

pub struct WelcomeDisplay<'a> {
    pub model: &'a str,
    pub provider: &'a str,
    pub auto_approve: bool,
    pub resumed: bool,
}

pub struct SessionStatus<'a> {
    pub model: &'a str,
    pub provider: &'a str,
    pub context: &'a str,
    pub auto_approve: bool,
}

/// Outcome of an approval prompt.
pub enum ApprovalResult {
    Approved,
    Denied { reason: String },
}

/// Inputs for the bash-command approval prompt.
pub struct BashApproval<'a> {
    pub command: &'a str,
    pub tier: RiskTier,
    pub reasons: &'a [String],
}

/// Inputs for rendering a finished tool execution line.
pub struct ToolLine<'a> {
    pub name: &'a str,
    pub arguments: &'a serde_json::Value,
    pub is_error: bool,
    pub output_summary: &'a str,
}

/// Inputs for rendering a tool-completion summary.
pub struct ToolOutcome<'a> {
    pub name: &'a str,
    pub is_error: bool,
    pub output_summary: &'a str,
}
