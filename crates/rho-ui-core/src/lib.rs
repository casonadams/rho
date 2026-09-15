//! Platform-agnostic reactive UI core for rho.
//!
//! Provides view models, Semantic UI Block IR, state hooks (Dioxus signals),
//! markdown/diff tokenizers, autocomplete engine, and prompt history
//! shared by the native Ratatui TUI and the Dioxus Web Hub.

pub mod autocomplete;
pub mod footer;
pub mod ir;
pub mod keymap;
pub mod modal;
pub mod parser;
pub mod permission;
pub mod session;
pub mod state;
pub mod text;
pub mod tree;

#[cfg(test)]
mod tests;

pub use autocomplete::{
    AutocompleteCandidate, BUILTIN_SLASH_COMMANDS, CompletionEngine, PromptHistory, SlashArgumentType, SlashCommandDef,
};
pub use footer::{abbreviate_home, get_git_branch};
pub use ir::{
    ChangeType, ContentBlock, DiagramKind, DiffHunk, HighlightedLine, ImageAttachment, InlineSpan, StyleToken,
    ThemeTokens, ToolInvocation,
};
pub use keymap::{InputAction, KeyAction, QueueKind, UiAction};
#[cfg(not(target_arch = "wasm32"))]
pub use keymap::{
    KeyChord, KeybindingMap, default_keybindings, map_app_action, map_key, map_key_with_bindings, parse_key_chord,
    parse_key_code,
};
pub use modal::{
    McpModalState, McpServerInfo, ModalOption, ModalState, ModelCapability, ModelRegistry, SettingsState, SkillInfo,
    SkillModalState,
};
pub use parser::{StreamChunkParser, StreamEvent, generate_diff, generate_word_diff, parse_markdown};
pub use permission::{PERMISSION_ACTIONS, PermissionAction, PermissionPromptState};
pub use session::{AuthMode, PROVIDER_DEFS, ProviderDef, SessionCommand, SessionState, SessionTurnState};
pub use state::{
    ActiveToolState, CompactionMilestone, FooterMetrics, PromptQueueCoordinator, QueuedPrompt, QueuedPromptKind,
    RhoTicket, SecretGuard, SessionTreeState, ToastLevel, ToastManager, ToastMessage, TreeNode, UpdateProgress,
    WelcomeDisplay, WindowFocus, detect_supported_image_mime, fit_dimensions, format_duration, format_duration_ms,
    format_relative_time, format_size, format_tokens,
};
pub use text::{
    fit_right_aligned, sanitize_status_text, strip_ansi, truncate_to_width, truncate_with_ellipsis, visible_width,
};
pub use tree::{TreeEntryDisplay, TreeItemInput, build_tree_display, render_tree_ascii};
