pub mod activity;
pub mod presenter;
pub mod stream;
pub mod structured;
pub mod summary;
pub mod types;

pub use activity::{ActivityToken, activity_token};
pub use presenter::Presenter;
pub use stream::{ToolStreamPort, ToolStreamSink};
pub use structured::{RecordingSink, StructuredOutputSink, StructuredPresenter};
pub use summary::summarize_tool_output;
pub use types::{
    BlockDisplay, FileChange, InteractionInput, InteractionOption, InteractionPrompt, InteractionResponse,
    OptionLayout, SessionStatus, ToolLine, TurnTelemetry, UI_EVENT_VERSION, UiEnvelope, UiEvent, WelcomeDisplay,
};

/// Starts the result of a subagent call that began in the background; the UI keys a hint on it.
pub const SUBAGENT_STARTED_MARKER: &str = "Started subagent task '";
