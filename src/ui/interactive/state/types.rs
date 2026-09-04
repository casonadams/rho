use super::editor::EditorState;
use super::modal::ModalState;

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
    Working,
}

impl Activity {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Working => "working",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FooterState {
    pub activity: Activity,
    pub running_tool: Option<String>,
    pub model: String,
    pub thinking_level: Option<String>,
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
    pub extra_status: Option<String>,
    pub hidden_status_count: usize,
    pub context: Option<String>,
    pub show_label: bool,
}

impl PartialEq for FooterState {
    fn eq(&self, other: &Self) -> bool {
        self.activity == other.activity
            && self.running_tool == other.running_tool
            && self.model == other.model
            && self.thinking_level == other.thinking_level
            && self.cwd == other.cwd
            && self.git_branch == other.git_branch
            && self.session_name == other.session_name
            && self.quota == other.quota
            && self.context_percent.map(f64::to_bits) == other.context_percent.map(f64::to_bits)
            && self.context_window == other.context_window
            && self.total_input_tokens == other.total_input_tokens
            && self.total_output_tokens == other.total_output_tokens
            && self.total_cache_read_tokens == other.total_cache_read_tokens
            && self.total_cache_write_tokens == other.total_cache_write_tokens
            && self.total_cost.map(f64::to_bits) == other.total_cost.map(f64::to_bits)
            && self.tokens_per_second.map(f64::to_bits) == other.tokens_per_second.map(f64::to_bits)
            && self.extra_status == other.extra_status
            && self.hidden_status_count == other.hidden_status_count
            && self.context == other.context
            && self.show_label == other.show_label
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
