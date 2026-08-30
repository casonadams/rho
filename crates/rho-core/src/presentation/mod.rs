//! Deterministic presentation contracts: render payload data, question and
//! streaming ports, formatters, and the presenter interface implemented by
//! terminal presentation plugins.

pub mod activity;

pub mod presenter;
pub mod questions;
pub mod stream;
pub mod summary;
pub mod types;

pub use activity::{ActivityToken, activity_token};
pub use presenter::Presenter;
pub use questions::{InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
pub use stream::{ToolStreamPort, ToolStreamSink};
pub use summary::summarize_tool_output;
pub use types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
