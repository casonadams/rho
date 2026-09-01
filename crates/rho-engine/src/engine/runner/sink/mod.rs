pub mod approval;
pub mod types;

pub use approval::{TerminalApprovalSink, TerminalSinkState, ToolFinishDetails};
pub use types::{CompletedTool, DisplayKind, TerminalSinkConfig, TurnArtifacts};
