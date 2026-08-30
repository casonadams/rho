//! Deterministic presentation contracts: the versioned render vocabulary
//! (published as `rho-sdk`'s `ui` module), the presenter interface implemented
//! by terminal presentation plugins, and the question/stream/activity ports
//! shared with tool dispatch.

pub use rho_sdk::ui::{
    ApprovalResult, BashApproval, RiskTier, SessionStatus, ToolLine, ToolOutcome, UI_EVENT_VERSION, UiEnvelope,
    UiEvent, WelcomeDisplay,
};

pub mod activity;
pub mod presenter;
pub mod questions;
pub mod stream;
pub mod summary;

pub use activity::{ActivityToken, activity_token};
pub use presenter::Presenter;
pub use questions::{InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
pub use stream::{ToolStreamPort, ToolStreamSink};
pub use summary::summarize_tool_output;
