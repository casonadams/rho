use crate::args::{AskUserArgs, BashArgs, EditArgs, FetchArgs, ReadArgs, SearchArgs, WriteArgs};
pub use crate::bash_ast::RiskTier;
use crate::bash_ast::analyze_command_safety;
use crate::workspace::Workspace;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionClass {
    ReadOnly,
    WorkspaceMutation,
    ApprovalRequired { tier: RiskTier, reasons: Vec<String> },
}

impl ExecutionClass {
    pub fn allows_without_approval(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::WorkspaceMutation)
    }

    fn mutating(reason: impl Into<String>) -> Self {
        Self::ApprovalRequired {
            tier: RiskTier::Mutating,
            reasons: vec![reason.into()],
        }
    }
}

pub struct ToolExecutionPolicy;

impl ToolExecutionPolicy {
    pub fn classify(tool_name: &str, arguments: &Value) -> ExecutionClass {
        let working_dir = std::env::current_dir().unwrap_or_default();
        Self::classify_in(tool_name, arguments, &working_dir)
    }

    fn classify_in(tool_name: &str, arguments: &Value, working_dir: &Path) -> ExecutionClass {
        if !is_known(tool_name) {
            return ExecutionClass::mutating("Unknown or malformed tool call cannot be proven read-only");
        }
        match tool_name {
            "read" if valid::<ReadArgs>(arguments) => ExecutionClass::ReadOnly,
            "websearch" | "web_search" if valid::<SearchArgs>(arguments) => ExecutionClass::ReadOnly,
            "webfetch" | "web_fetch" if valid::<FetchArgs>(arguments) => ExecutionClass::ReadOnly,
            "ask_user" | "ask_user_question" => ExecutionClass::ReadOnly,
            "agent" | "Agent" => ExecutionClass::ReadOnly,
            "get_subagent_result" => ExecutionClass::ReadOnly,
            "steer_subagent" => ExecutionClass::ReadOnly,
            "todo" | "Todo" => ExecutionClass::ReadOnly,
            "write" => classify_write(arguments, working_dir),
            "edit" => classify_edit(arguments, working_dir),
            "bash" => classify_bash(arguments),
            _ => ExecutionClass::mutating("Known tool arguments are malformed or unsafe"),
        }
    }

    pub fn canonical_arguments(tool_name: &str, arguments: &Value) -> Option<Value> {
        match tool_name {
            "read" => canonical::<ReadArgs>(arguments),
            "write" => canonical::<WriteArgs>(arguments),
            "edit" => canonical::<EditArgs>(arguments),
            "bash" => canonical::<BashArgs>(arguments),
            "websearch" | "web_search" => canonical::<SearchArgs>(arguments),
            "webfetch" | "web_fetch" => canonical::<FetchArgs>(arguments),
            "ask_user" | "ask_user_question" => canonical::<AskUserArgs>(arguments),
            _ => Some(arguments.clone()),
        }
    }
}

fn canonical<T>(arguments: &Value) -> Option<Value>
where
    T: DeserializeOwned + serde::Serialize,
{
    serde_json::from_value::<T>(arguments.clone())
        .ok()
        .and_then(|arguments| serde_json::to_value(arguments).ok())
}

fn valid<T: DeserializeOwned>(arguments: &Value) -> bool {
    serde_json::from_value::<T>(arguments.clone()).is_ok()
}

fn classify_write(arguments: &Value, working_dir: &Path) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<WriteArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed write arguments cannot be validated safely");
    };
    let workspace = Workspace::new(working_dir);
    classify_file_path(&args.path, &workspace, "Write target exits the working directory")
}

fn classify_edit(arguments: &Value, working_dir: &Path) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<EditArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed edit arguments cannot be validated safely");
    };
    classify_file_path(
        &args.path,
        &Workspace::new(working_dir),
        "Edit target exits the working directory",
    )
}

fn classify_file_path(path: &str, workspace: &Workspace, outside_reason: &str) -> ExecutionClass {
    if workspace.is_protected(path) {
        return ExecutionClass::mutating("Target is protected repository metadata: .git");
    }
    if workspace.is_within(path) {
        ExecutionClass::WorkspaceMutation
    } else {
        ExecutionClass::mutating(outside_reason)
    }
}

fn classify_bash(arguments: &Value) -> ExecutionClass {
    let Ok(args) = serde_json::from_value::<BashArgs>(arguments.clone()) else {
        return ExecutionClass::mutating("Malformed bash arguments cannot be analyzed safely");
    };
    let analysis = analyze_command_safety(&args.command);
    if analysis.tier == RiskTier::ReadOnly {
        ExecutionClass::ReadOnly
    } else {
        ExecutionClass::ApprovalRequired {
            tier: analysis.tier,
            reasons: analysis.reasons,
        }
    }
}

pub fn is_known(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read"
            | "write"
            | "edit"
            | "bash"
            | "websearch"
            | "web_search"
            | "webfetch"
            | "web_fetch"
            | "ask_user"
            | "ask_user_question"
            | "agent"
            | "Agent"
            | "get_subagent_result"
            | "steer_subagent"
            | "todo"
            | "Todo"
    )
}
