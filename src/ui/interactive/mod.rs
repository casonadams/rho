pub mod a2ui;
pub(crate) mod controller;
mod events;
pub mod footer;
mod input;
mod layout;
mod pacer;
pub mod session_picker;
#[cfg(test)]
mod shift_tab_tests;
mod state;
mod transcript;

#[cfg(test)]
pub(crate) use controller::NullBackend;
pub use controller::cache;
pub use controller::view::ViewFrame;
pub use controller::{CrosstermBackend, TerminalBackend, TerminalController};
pub use events::{
    BatchDecision, FlushBarrier, InteractionInput, InteractionOption, InteractionPrompt, InteractionResponder,
    InteractionResponse, InteractiveUi, OptionLayout, OutputEvent, PendingUiBatch, PendingUiDrain, ToolStartRequest,
    UiEvent, UiPortError,
};
pub use footer::{
    abbreviate_home, fit_right_aligned, format_footer_lines, format_stats_line, format_tokens, format_top_line,
    get_git_branch, get_git_status_counts, get_git_status_counts_async, sanitize_status_text,
};
pub use input::{InputAction, map_key};
pub use layout::{
    CursorPosition, InteractiveLayout, LayoutInput, SPINNER_FRAMES, VisualTruncateResult, layout,
    modal_body_max_scroll, truncate_to_visual_lines, wrap_to_width,
};
pub use state::{
    Activity, AutocompleteItem, AutocompleteState, EditorState, FooterState, GitPrInfo, GitStatusCounts,
    InteractiveState, MAX_RUNNING_BUFFER_BYTES, MAX_RUNNING_OUTPUT_BYTES, ModalMode, ModalOption, ModalState,
    QueueKind, QueuedMessage, RunningTool, UiAction, UiEffect,
};
pub use transcript::{
    ToolItem, TranscriptItem, TranscriptRenderInput, WelcomeItem, format_recap_notice, format_welcome_content,
    render_transcript_item,
};

#[cfg(test)]
pub(crate) use transcript::tool_fixture;
