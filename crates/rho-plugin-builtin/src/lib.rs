//! Built-in capability plugin implementations: tools, commands, skills, and MCP.

pub mod commands;
pub mod mcp;
pub mod skills;
pub mod subagents;
pub mod tools;

pub use commands::BuiltinCommand;
pub use skills::BuiltinSkillCapability;
pub use tools::{
    AskUserArgs, AskUserQuestionTool, AskUserTool, BashArgs, BashTool, BuiltinToolCatalog, BuiltinToolDeclaration,
    BuiltinToolKind, DECLARATIONS, EditArgs, EditTool, ReadArgs, ReadTool, TaskStatus, TodoAction, TodoArgs, TodoStore,
    TodoTask, TodoTool, ToolRegistry, ToolResult, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
    WriteArgs, WriteTool,
};
