use crate::tools::ask_user::AskUserArgs;
use crate::tools::bash::BashArgs;
use crate::tools::edit::EditArgs;
use crate::tools::read::ReadArgs;
pub use crate::tools::todo::PROMPT_TODO;
use crate::tools::todo::TodoArgs;
use crate::tools::types::generated_schema;
use crate::tools::web::fetch::FetchArgs;
use crate::tools::web::search::SearchArgs;
use crate::tools::write::WriteArgs;
use rho_sdk::contract::{ExecutionMode, NetworkAccess, OperationEffect, PathScope, ToolDescriptor};

pub static PROMPT_READ: &str = include_str!("../../../../../prompts/tools/read.md");
pub static PROMPT_WRITE: &str = include_str!("../../../../../prompts/tools/write.md");
pub static PROMPT_EDIT: &str = include_str!("../../../../../prompts/tools/edit.md");
pub static PROMPT_BASH: &str = include_str!("../../../../../prompts/tools/bash.md");
pub static PROMPT_ASK_USER: &str = include_str!("../../../../../prompts/tools/ask_user.md");
pub static PROMPT_WEBSEARCH: &str = include_str!("../../../../../prompts/tools/websearch.md");
pub static PROMPT_WEBFETCH: &str = include_str!("../../../../../prompts/tools/webfetch.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Interactive,
    Shell,
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinToolDeclaration {
    pub name: &'static str,
    pub capability: BuiltinToolKind,
    pub description: &'static str,
    pub prompt: &'static str,
    pub(crate) schema: fn() -> serde_json::Value,
    pub(crate) effects: &'static [OperationEffect],
    pub execution_mode: ExecutionMode,
}

impl BuiltinToolDeclaration {
    pub fn descriptor(self) -> ToolDescriptor {
        ToolDescriptor {
            id: format!("tool:{}", self.name).parse().unwrap(),
            description: self.description.to_string(),
            argument_schema: (self.schema)(),
            prompt_guidance: self.prompt.to_string(),
            effects: self.effects.to_vec(),
            execution_mode: self.execution_mode,
        }
    }
}

const READ_EFFECTS: &[OperationEffect] = &[OperationEffect::ReadPath {
    scope: PathScope::Explicit,
}];
const WRITE_EFFECTS: &[OperationEffect] = &[OperationEffect::WritePath {
    scope: PathScope::Workspace,
}];
const BASH_EFFECTS: &[OperationEffect] = &[OperationEffect::ExecuteProcess];
const NETWORK_EFFECTS: &[OperationEffect] = &[OperationEffect::Network {
    access: NetworkAccess::PublicInternet,
}];
const INTERACTION_EFFECTS: &[OperationEffect] = &[OperationEffect::UserInteraction];

pub const DECLARATIONS: &[BuiltinToolDeclaration] = &[
    BuiltinToolDeclaration {
        name: "read",
        capability: BuiltinToolKind::ReadOnly,
        description: "Read file contents with line numbering, offset, and limit safeguards.",
        prompt: PROMPT_READ,
        schema: generated_schema::<ReadArgs>,
        effects: READ_EFFECTS,
        execution_mode: ExecutionMode::Parallel,
    },
    BuiltinToolDeclaration {
        name: "write",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Write full content to a file, automatically creating parent directories.",
        prompt: PROMPT_WRITE,
        schema: generated_schema::<WriteArgs>,
        effects: WRITE_EFFECTS,
        execution_mode: ExecutionMode::Sequential,
    },
    BuiltinToolDeclaration {
        name: "edit",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Edit a file by applying exact string replacements. Every oldText must match exactly once.",
        prompt: PROMPT_EDIT,
        schema: generated_schema::<EditArgs>,
        effects: WRITE_EFFECTS,
        execution_mode: ExecutionMode::Sequential,
    },
    BuiltinToolDeclaration {
        name: "bash",
        capability: BuiltinToolKind::Shell,
        description: "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        prompt: PROMPT_BASH,
        schema: generated_schema::<BashArgs>,
        effects: BASH_EFFECTS,
        execution_mode: ExecutionMode::Sequential,
    },
    BuiltinToolDeclaration {
        name: "websearch",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        schema: generated_schema::<SearchArgs>,
        effects: NETWORK_EFFECTS,
        execution_mode: ExecutionMode::Parallel,
    },
    BuiltinToolDeclaration {
        name: "web_search",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        schema: generated_schema::<SearchArgs>,
        effects: NETWORK_EFFECTS,
        execution_mode: ExecutionMode::Parallel,
    },
    BuiltinToolDeclaration {
        name: "webfetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        schema: generated_schema::<FetchArgs>,
        effects: NETWORK_EFFECTS,
        execution_mode: ExecutionMode::Parallel,
    },
    BuiltinToolDeclaration {
        name: "web_fetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        schema: generated_schema::<FetchArgs>,
        effects: NETWORK_EFFECTS,
        execution_mode: ExecutionMode::Parallel,
    },
    BuiltinToolDeclaration {
        name: "ask_user",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        schema: generated_schema::<AskUserArgs>,
        effects: INTERACTION_EFFECTS,
        execution_mode: ExecutionMode::Sequential,
    },
    BuiltinToolDeclaration {
        name: "ask_user_question",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        schema: generated_schema::<AskUserArgs>,
        effects: INTERACTION_EFFECTS,
        execution_mode: ExecutionMode::Sequential,
    },
    BuiltinToolDeclaration {
        name: "todo",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Manage a task list for tracking multi-step progress. Actions: create, update, list, get, delete, clear.",
        prompt: PROMPT_TODO,
        schema: generated_schema::<TodoArgs>,
        effects: &[],
        execution_mode: ExecutionMode::Sequential,
    },
];

impl PartialEq for BuiltinToolDeclaration {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.capability == other.capability
            && self.description == other.description
            && self.prompt == other.prompt
            && std::ptr::fn_addr_eq(self.schema, other.schema)
            && self.effects == other.effects
            && self.execution_mode == other.execution_mode
    }
}

impl Eq for BuiltinToolDeclaration {}
