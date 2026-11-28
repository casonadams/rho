use super::{AgentRunner, ToolObserver};
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::TurnStore;
use crate::engine::runner::turn::types::{CancellationSignal, SteeringQueueProvider};
use crate::hook::LifecycleHook;
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::SessionManager;
use rho_harness_core::thinking::ThinkingLevel;
use std::sync::Arc;

impl AgentRunner {
    pub fn new(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        session_manager: SessionManager,
        sink: Arc<TerminalApprovalSink>,
    ) -> Self {
        let working_dir = session_manager
            .file_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let store: Arc<dyn TurnStore> = Arc::new(session_manager.clone());
        Self::assemble(model, tools, store, sink, working_dir).session(Some(session_manager))
    }

    /// A runner whose conversation lives in `store` (an agent with no session file of its own),
    /// working in `working_dir`.
    pub fn with_store(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        store: Arc<dyn TurnStore>,
        sink: Arc<TerminalApprovalSink>,
        working_dir: std::path::PathBuf,
    ) -> Self {
        Self::assemble(model, tools, store, sink, working_dir)
    }

    fn assemble(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        store: Arc<dyn TurnStore>,
        sink: Arc<TerminalApprovalSink>,
        working_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            model,
            tools,
            store,
            session_manager: None,
            preamble: String::new(),
            prompt: String::new(),
            budget: 10,
            history: Vec::new(),
            sink,
            repeat_tracker: RepeatedCallTracker::default(),
            lifecycle: None,
            permission: None,
            steering: None,
            inbox: None,
            cancellation: None,
            tool_observer: None,
            working_dir,
            monitor: None,
            project: None,
            worktrees: None,
            context_budget: None,
            a2ui_widgets: false,
            a2ui_config: rho_harness_core::config::A2uiConfig::default(),
            thinking_level: None,
            conversation_id: None,
        }
    }

    pub fn preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = preamble.into();
        self
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self
    }

    pub fn history(mut self, history: Vec<ChatMessage>) -> Self {
        self.history = history;
        self
    }

    /// Keeps the turn's conversation in `store` instead of the session, and makes the store's owner
    /// own the turn's background work. The session still reaches tools through their context.
    pub fn store(mut self, store: Arc<dyn TurnStore>) -> Self {
        self.store = store;
        self
    }

    /// The session the turn's tools see (where subagent tasks are saved).
    pub fn session(mut self, session: Option<SessionManager>) -> Self {
        self.session_manager = session;
        self
    }

    /// Tells `observer` how each tool call ended: refused, or run with the output the model receives.
    pub fn observer(mut self, observer: Option<Arc<dyn ToolObserver>>) -> Self {
        self.tool_observer = observer;
        self
    }

    #[cfg(test)]
    pub(super) fn session_file(&self) -> &SessionManager {
        self.session_manager.as_ref().expect("test runners have a session")
    }

    pub fn lifecycle(mut self, lifecycle: Option<Arc<LifecycleHook>>) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub fn permission(mut self, permission: Option<Arc<crate::permission::PermissionHook>>) -> Self {
        self.permission = permission;
        self
    }

    pub fn steering(mut self, steering: Option<Arc<dyn SteeringQueueProvider>>) -> Self {
        self.steering = steering;
        self
    }

    /// The agent's mailbox: read before each model call, and handed to its tools so the background
    /// work they start reports back to it.
    pub fn inbox(mut self, inbox: Option<crate::mailbox::Mailbox>) -> Self {
        self.inbox = inbox;
        self
    }

    pub fn cancellation(mut self, cancellation: Option<CancellationSignal>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn monitor(mut self, monitor: Option<rho_harness_core::agents::AgentMonitor>) -> Self {
        self.monitor = monitor;
        self
    }

    pub fn project_context(mut self, project: Option<Arc<crate::engine::context::ProjectContext>>) -> Self {
        self.project = project;
        self
    }

    /// Reminds the model, once per turn, to merge or discard the worktrees its session still owns.
    pub fn worktrees(mut self, store: Option<crate::worktree::WorktreeStore>) -> Self {
        self.worktrees = store;
        self
    }

    /// Tells the model how full its context window is, once per notice percentage it crosses.
    pub(crate) fn context_budget(mut self, budget: Option<crate::engine::tracking::ContextBudget>) -> Self {
        self.context_budget = budget;
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<std::path::PathBuf>) -> Self {
        self.working_dir = working_dir.into();
        self
    }

    pub fn a2ui_config(mut self, enabled: bool, config: rho_harness_core::config::A2uiConfig) -> Self {
        self.a2ui_widgets = enabled;
        self.a2ui_config = config;
        self
    }

    pub fn thinking_level(mut self, thinking_level: Option<ThinkingLevel>) -> Self {
        self.thinking_level = thinking_level;
        self
    }

    /// Identifies the conversation every request of this runner belongs to.
    pub fn conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self
    }
}
