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
pub mod wait;
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

/// Shared, dynamically-relocatable workspace root for a session's tools.
///
/// Built-in tools for a session share one `WorkspaceDir` handle so that
/// `worktree(action="create")` takes effect immediately on the very next tool
/// call within the same turn. Subagents always receive their own independent
/// `WorkspaceDir::new(...)` so relocating the parent never moves a running subagent.
#[derive(Clone, Debug)]
pub struct WorkspaceDir(std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>);

impl WorkspaceDir {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self(std::sync::Arc::new(std::sync::RwLock::new(path.into())))
    }

    #[must_use]
    pub fn get(&self) -> std::path::PathBuf {
        self.0.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn set(&self, path: impl Into<std::path::PathBuf>) {
        let mut guard = self.0.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = path.into();
    }
}
