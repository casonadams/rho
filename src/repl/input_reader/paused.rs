use std::io;
use std::sync::mpsc as std_mpsc;

use super::{TerminalInputReader, reader_stopped};

pub(super) enum Control {
    Pause(std_mpsc::SyncSender<()>),
    Resume,
    Stop,
}

pub(crate) struct PausedInput<'a> {
    pub(super) reader: &'a TerminalInputReader,
    pub(super) resumed: bool,
}

impl PausedInput<'_> {
    pub(crate) fn resume(mut self) -> io::Result<()> {
        self.resumed = true;
        self.reader.control.send(Control::Resume).map_err(|_| reader_stopped())
    }
}

impl Drop for PausedInput<'_> {
    fn drop(&mut self) {
        if !self.resumed {
            let _ = self.reader.control.send(Control::Resume);
        }
    }
}
