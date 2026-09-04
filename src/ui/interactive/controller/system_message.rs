use std::time::{Duration, Instant};

use super::TerminalController;
use super::backend::TerminalBackend;

pub const SYSTEM_MESSAGE_DURATION: Duration = Duration::from_secs(3);

impl<B: TerminalBackend> TerminalController<B> {
    pub fn set_system_message(&mut self, message: impl Into<String>) {
        self.state.set_system_message(Some(message.into()));
        self.system_message_expires_at = Some(Instant::now() + SYSTEM_MESSAGE_DURATION);
    }

    pub fn clear_system_message(&mut self) {
        self.state.set_system_message(None);
        self.system_message_expires_at = None;
    }

    pub fn check_system_message_expiration(&mut self) -> bool {
        if let Some(expires_at) = self.system_message_expires_at
            && Instant::now() >= expires_at
        {
            self.clear_system_message();
            return true;
        }
        false
    }
}
