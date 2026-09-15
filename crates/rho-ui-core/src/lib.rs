//! Platform-agnostic reactive UI core for rho.
//!
//! Provides view models, Semantic UI Block IR, state hooks (Dioxus signals),
//! markdown/diff tokenizers, autocomplete engine, and prompt history
//! shared by the native Ratatui TUI and the Dioxus Web Hub.

pub mod autocomplete;
pub mod ir;
pub mod modal;
pub mod parser;
pub mod permission;
pub mod session;
pub mod state;

#[cfg(test)]
mod tests;

pub use autocomplete::{
    AutocompleteCandidate, BUILTIN_SLASH_COMMANDS, CompletionEngine, PromptHistory, SlashArgumentType, SlashCommandDef,
};
pub use ir::{
    ChangeType, ContentBlock, DiagramKind, DiffHunk, HighlightedLine, ImageAttachment, InlineSpan, StyleToken,
    ThemeTokens, ToolInvocation,
};
pub use modal::{
    McpModalState, McpServerInfo, ModalOption, ModalState, ModelCapability, ModelRegistry, SettingsState, SkillInfo,
    SkillModalState,
};
pub use parser::{StreamChunkParser, StreamEvent, generate_diff, generate_word_diff, parse_markdown};
pub use permission::{PERMISSION_ACTIONS, PermissionAction, PermissionPromptState};
pub use session::{AuthMode, PROVIDER_DEFS, ProviderDef, SessionCommand, SessionState, SessionTurnState};
pub use state::{
    FooterMetrics, PromptQueueCoordinator, QueuedPrompt, QueuedPromptKind, RhoTicket, SecretGuard, SessionTreeState,
    ToastLevel, ToastManager, ToastMessage, TreeNode, WelcomeDisplay, WindowFocus, detect_supported_image_mime,
    fit_dimensions, format_size, format_tokens,
};
