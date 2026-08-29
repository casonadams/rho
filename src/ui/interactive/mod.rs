mod controller;
mod events;
mod input;
mod layout;
mod state;

pub use controller::{CrosstermBackend, TerminalBackend, TerminalController};
pub use events::{
    BatchDecision, FlushBarrier, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse,
    InteractiveUi, OutputEvent, PendingUiBatch, PendingUiDrain, UiEvent, UiPortError,
};
pub use input::{InputAction, map_key};
pub use layout::{CursorPosition, InteractiveLayout, LayoutInput, RunningToolDisplay, layout};
pub use state::{
    Activity, EditorState, FooterState, InteractiveState, ModalMode, ModalOption, ModalState, QueueKind, QueuedMessage,
    RunningTool, UiAction, UiEffect,
};
