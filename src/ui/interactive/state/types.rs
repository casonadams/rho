use super::editor::EditorState;
use super::modal::ModalState;
use rho_harness_core::config::{PromptUiConfig, StatusBarConfig};
use rho_harness_core::thinking::ThinkingLevel;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GitStatusCounts {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPrInfo {
    pub number: u64,
    pub url: Option<String>,
}

impl GitStatusCounts {
    pub fn is_dirty(&self) -> bool {
        self.staged > 0 || self.unstaged > 0 || self.untracked > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueKind {
    Steering,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub kind: QueueKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Thinking,
    Compacting,
    Working,
}

impl Activity {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Compacting => "compacting",
            Self::Working => "working",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FooterState {
    pub activity: Activity,
    pub running_tool: Option<String>,
    pub provider: String,
    pub model: String,
    pub thinking_level: Option<String>,
    pub resolved_thinking_level: Option<ThinkingLevel>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub session_name: Option<String>,
    pub quota: Option<String>,
    pub context_percent: Option<f64>,
    pub context_window: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_cost: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub hidden_status_count: usize,
    pub context: Option<String>,
    pub show_label: bool,
    /// Subagents whose rounds are running; shown so the user knows to look with Left.
    pub running_agents: usize,
    /// The agent shown in place of the main agent, as its footer title.
    pub viewing: Option<String>,
    /// The agent, not shown, whose permission prompt is waiting for the user.
    pub awaiting_approval: Option<String>,
    pub git_status: Option<GitStatusCounts>,
    pub git_pr: Option<GitPrInfo>,
    pub session_id: Option<String>,
    pub hostname: Option<String>,
    pub active_duration_ms: u64,
    pub prompt_config: Option<PromptUiConfig>,
    pub status_bar_config: Option<StatusBarConfig>,
}

type IdentityKey<'a> = (
    &'a Activity,
    Option<&'a str>,
    &'a str,
    &'a str,
    Option<&'a str>,
    Option<ThinkingLevel>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

type MetricsKey<'a> = (
    Option<&'a str>,
    Option<u64>,
    usize,
    u64,
    u64,
    u64,
    u64,
    Option<u64>,
    Option<u64>,
);

type UiKey<'a> = (usize, Option<&'a str>, bool, usize, Option<&'a str>, Option<&'a str>);

type StatusBarKey<'a> = (
    Option<GitStatusCounts>,
    Option<&'a GitPrInfo>,
    Option<&'a str>,
    Option<&'a str>,
    u64,
    Option<&'a PromptUiConfig>,
    Option<&'a StatusBarConfig>,
);

impl FooterState {
    fn identity_key(&self) -> IdentityKey<'_> {
        (
            &self.activity,
            self.running_tool.as_deref(),
            &self.provider,
            &self.model,
            self.thinking_level.as_deref(),
            self.resolved_thinking_level,
            self.cwd.as_deref(),
            self.git_branch.as_deref(),
            self.session_name.as_deref(),
        )
    }

    fn metrics_key(&self) -> MetricsKey<'_> {
        (
            self.quota.as_deref(),
            self.context_percent.map(f64::to_bits),
            self.context_window,
            self.total_input_tokens,
            self.total_output_tokens,
            self.total_cache_read_tokens,
            self.total_cache_write_tokens,
            self.total_cost.map(f64::to_bits),
            self.tokens_per_second.map(f64::to_bits),
        )
    }

    fn ui_key(&self) -> UiKey<'_> {
        (
            self.hidden_status_count,
            self.context.as_deref(),
            self.show_label,
            self.running_agents,
            self.viewing.as_deref(),
            self.awaiting_approval.as_deref(),
        )
    }

    fn status_bar_key(&self) -> StatusBarKey<'_> {
        (
            self.git_status,
            self.git_pr.as_ref(),
            self.session_id.as_deref(),
            self.hostname.as_deref(),
            self.active_duration_ms,
            self.prompt_config.as_ref(),
            self.status_bar_config.as_ref(),
        )
    }
}

impl PartialEq for FooterState {
    fn eq(&self, other: &Self) -> bool {
        self.identity_key() == other.identity_key()
            && self.metrics_key() == other.metrics_key()
            && self.ui_key() == other.ui_key()
            && self.status_bar_key() == other.status_bar_key()
    }
}

impl Eq for FooterState {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Insert(char),
    InsertNewline,
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveWordLeft,
    MoveWordRight,
    MoveToStart,
    MoveToEnd,
    DeleteWordBackward,
    DeleteWordForward,
    DeleteToLineStart,
    DeleteToLineEnd,
    Yank,
    Undo,
    Paste(String),
    Submit(QueueKind),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    None,
    Queued(QueuedMessage),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalFrame {
    pub(crate) modal: ModalState,
    pub(crate) saved_editor: EditorState,
}
