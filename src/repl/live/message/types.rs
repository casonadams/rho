use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) struct LiveCommandContext<'a, 'b> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
}

pub(crate) struct SessionCommandIo<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub input: &'a mut TerminalInputReader,
}

pub(crate) struct BranchSwitchContext<'a, 'b, 'c, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'c mut TerminalController<B>,
    pub history: &'c mut InteractiveHistory,
    pub input: &'c mut TerminalInputReader,
}
