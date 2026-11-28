use crate::lsp::LspManager;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

pub(crate) struct FileTool<Op> {
    pub base_dir: crate::tools::WorkspaceDir,
    pub lsp: Option<Arc<LspManager>>,
    pub history: Option<crate::tools::undo::EditHistory>,
    /// The agent's mailbox, where diagnostics that arrive late are delivered.
    pub(crate) inbox: Option<crate::mailbox::Mailbox>,
    op: PhantomData<Op>,
}

impl<Op> FileTool<Op> {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: crate::tools::WorkspaceDir::new(base_dir.as_ref()),
            lsp: None,
            history: None,
            inbox: None,
            op: PhantomData,
        }
    }

    pub(crate) fn with_workspace_dir(mut self, base_dir: crate::tools::WorkspaceDir) -> Self {
        self.base_dir = base_dir;
        self
    }

    pub(crate) fn with_lsp(mut self, lsp: Option<Arc<LspManager>>) -> Self {
        self.lsp = lsp;
        self
    }

    pub fn with_history(mut self, history: Option<crate::tools::undo::EditHistory>) -> Self {
        self.history = history;
        self
    }

    pub(crate) fn bound_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>>
    where
        Self: EngineTool + 'static,
    {
        self.lsp.as_ref()?;
        Some(Arc::new(Self {
            base_dir: self.base_dir.clone(),
            lsp: self.lsp.clone(),
            history: self.history.clone(),
            inbox: Some(ctx.inbox.clone()?),
            op: PhantomData,
        }))
    }
}
