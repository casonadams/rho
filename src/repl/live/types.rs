use tokio::sync::mpsc;

use crate::repl::ReplSession;
pub(crate) use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{QueuedMessage, TerminalController, UiEvent};

pub struct LiveIo<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
    pub input: &'a mut TerminalInputReader,
}

pub struct EditorResources<'a> {
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
}

pub struct LiveMessage<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub message: QueuedMessage,
}

pub struct ActiveTurn<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub prompt: &'a str,
}

pub struct IdleContext<'a, 'b, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub session: &'b mut ReplSession,
    pub engine: &'b mut crate::engine::AgentEngine,
}

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}
