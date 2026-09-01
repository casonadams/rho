pub mod activity;
pub mod presenter;
pub mod questions;
pub mod stream;
pub mod structured;
pub mod summary;
pub mod types;

pub use activity::{ActivityToken, activity_token};
pub use presenter::Presenter;
pub use questions::{InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
pub use stream::{ToolStreamPort, ToolStreamSink};
pub use structured::{RecordingSink, StdoutNdjsonSink, StructuredOutputSink, StructuredPresenter};
pub use summary::summarize_tool_output;
pub use types::{
    ApprovalResult, BashApproval, RiskTier, SessionStatus, ToolLine, ToolOutcome, UI_EVENT_VERSION, UiEnvelope,
    UiEvent, WelcomeDisplay,
};
