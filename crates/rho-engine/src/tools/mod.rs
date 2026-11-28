//! Built-in tools and shared plumbing. Each tool directory owns its argument schema, execution, and
//! truncation; `registry` wires declarations into the model-facing tool list.

pub mod artifact;
pub mod ask;
pub(crate) mod atomic;
pub mod bash;
pub mod builtin_tools;
pub mod edit;
pub mod engine_tool;
pub mod fd;
pub mod file_change;
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

pub(crate) use ask::AskTool;
pub use bash::{BashArgs, BashTool};
pub use builtin_tools::{BuiltinTools, build_builtin_tools};
pub use edit::EditArgs;
pub use engine_tool::EngineTool;
pub(crate) use engine_tool::GenericTool;
pub use fd::{FdArgs, FdSort};
pub use jobs::{BackgroundJobInfo, JobManager, JobStatus};
pub(crate) use memory::MemoryTool;
pub use read::ReadArgs;
pub use registry::ToolRegistry;
pub use rg::RgArgs;
pub use rho_harness_core::args::{WebFetchArgs, WebSearchArgs};
pub use rho_harness_core::net::HttpRequest;
pub use subagent::{SubagentArgs, SubagentRole, SubagentRunner, SubagentTool};
pub use traversal::list_workspace_files;
pub use types::ToolResult;
pub(crate) use types::normalize_schema;
pub use web::{HttpClient, WebFetchConfig, WebSearchConfig};
pub use workflow::{WorkflowRun, WorkflowRunner};
pub(crate) use worktree_tool::WorktreeTool;
pub use write::WriteArgs;

/// Workspace root shared by a session's tools and relocatable mid-turn (`worktree(action="create")`).
/// Each subagent gets its own handle, so relocating the parent never moves a running subagent.
#[derive(Clone, Debug)]
pub struct WorkspaceDir(std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>);

impl WorkspaceDir {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self(std::sync::Arc::new(std::sync::RwLock::new(path.into())))
    }

    #[must_use]
    pub fn get(&self) -> std::path::PathBuf {
        crate::locking::read(&self.0).clone()
    }

    pub fn set(&self, path: impl Into<std::path::PathBuf>) {
        *crate::locking::write(&self.0) = path.into();
    }
}
