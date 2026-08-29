mod controller;
mod events;
mod input;
mod layout;
mod state;
mod transcript;

pub use controller::{CrosstermBackend, TerminalBackend, TerminalController};
pub use events::{
    BatchDecision, FlushBarrier, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse,
    InteractiveUi, OutputEvent, PendingUiBatch, PendingUiDrain, UiEvent, UiPortError,
};
pub use input::{InputAction, map_key};
pub use layout::{
    ActiveToolDisplayInput, CursorPosition, InteractiveLayout, LayoutInput, SPINNER_FRAMES, VisualTruncateResult,
    format_active_tool_block, layout, truncate_to_visual_lines, wrap_to_width,
};
pub use state::{
    Activity, EditorState, FooterState, InteractiveState, ModalMode, ModalOption, ModalState, QueueKind, QueuedMessage,
    RunningTool, UiAction, UiEffect,
};
pub use transcript::{ToolItem, TranscriptItem, TranscriptRenderInput, WelcomeItem, render_transcript_item};
