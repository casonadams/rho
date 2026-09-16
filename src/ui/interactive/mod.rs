mod events;
pub mod footer;
mod input;
pub mod key_parser;
pub mod keybinding_loader;
pub mod keymap;
#[cfg(test)]
mod shift_tab_tests;
mod state;
mod transcript;
pub mod tree_view {
    pub use rho_harness_core::session::tree::{TreeEntryDisplay, build_tree_display, render_tree_ascii};
}

pub use events::{
    InteractionInput, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse, InteractiveUi,
    OptionLayout, OutputEvent, ToolStartRequest, UiEvent, UiPortError,
};
pub use footer::{
    abbreviate_home, fit_right_aligned, format_footer_lines, format_stats_line, format_tokens, format_top_line,
    get_git_branch, sanitize_status_text,
};
pub use input::{InputAction, map_key, map_key_with_bindings};
pub use keybinding_loader::{default_keybindings, load_keybindings};
pub use keymap::{KeyAction, KeyChord, KeybindingMap};
pub use state::{
    Activity, AutocompleteItem, AutocompleteState, EditorState, FooterState, InteractiveState,
    MAX_RUNNING_BUFFER_BYTES, MAX_RUNNING_OUTPUT_BYTES, ModalMode, ModalOption, ModalState, PasteStore, QueueKind,
    QueuedMessage, RunningTool, UiAction, UiEffect, check_paste_threshold, sanitize_paste,
};
pub use transcript::{
    ToolItem, TranscriptItem, TranscriptRenderInput, WelcomeItem, format_welcome_content, render_tool_block,
    render_transcript_item,
};
