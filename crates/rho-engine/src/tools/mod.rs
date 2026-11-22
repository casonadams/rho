//! Built-in tools and shared plumbing. Each tool directory owns its argument
//! schema (`rho_harness_core::args`), execution, and truncation behavior;
//! `registry` wires declarations into the model-facing tool list.

pub mod artifact;
pub(crate) mod atomic;
pub mod bash;
pub mod builtin_tools;
pub mod edit;
pub mod engine_tool;
pub mod fd;
pub mod jobs;
pub mod listing;
pub mod lsp;
pub mod memory;
pub mod read;
pub mod registry;
pub mod rg;
pub mod subagent;
pub mod suggest;
pub mod todo;
pub(crate) mod traversal;
pub mod truncate;
pub mod types;
pub mod undo;
pub mod web;
pub mod workflow;
pub mod worktree_tool;
pub mod write;

pub use artifact::{cleanup_artifacts, format_artifact_notice, spill_artifact};
pub use bash::{BashArgs, BashTool};
pub use builtin_tools::{BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, build_builtin_tools};
pub use edit::{EditArgs, EditTool};
pub use engine_tool::{EngineTool, GenericTool};
pub use fd::{FdArgs, FdSort, FdTool};
pub use jobs::{BackgroundJobInfo, JobManager, JobStatus};
pub use memory::MemoryTool;
pub use read::{ReadArgs, ReadTool};
pub use registry::ToolRegistry;
pub use rg::{RgArgs, RgTool};
pub use rho_harness_core::args::{WebFetchArgs, WebSearchArgs};
pub use rho_harness_core::net::HttpRequest;
pub use subagent::{SubagentArgs, SubagentRole, SubagentRunner, SubagentTool};
pub use todo::TodoTool;
pub use traversal::list_workspace_files;
pub use types::{ToolResult, generated_schema, normalize_schema};
pub use web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
pub use workflow::{WorkflowRun, WorkflowRunner};
pub use worktree_tool::WorktreeTool;
pub use write::{WriteArgs, WriteTool};
