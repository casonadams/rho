pub mod activity;
pub mod presenter;
pub mod stream;
pub mod structured;
pub mod summary;
pub mod transformer;
pub mod types;

pub use activity::{ActivityToken, activity_token};
pub use presenter::Presenter;
pub use stream::{ToolStreamPort, ToolStreamSink};
pub use structured::{RecordingSink, StdoutNdjsonSink, StructuredOutputSink, StructuredPresenter};
pub use summary::summarize_tool_output;
pub use transformer::{DisplayTransformer, DisplayTransformerPipeline, ReplaceTransformer};
pub use types::{
    BlockDisplay, InteractionInput, InteractionOption, InteractionPrompt, InteractionResponse, SessionStatus, ToolLine,
    ToolOutcome, UI_EVENT_VERSION, UiEnvelope, UiEvent, WelcomeDisplay,
};
