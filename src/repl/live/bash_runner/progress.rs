use std::time::{Duration, Instant};

use crate::repl::live::batch::SPINNER_FRAME_INTERVAL;
use crate::ui::interactive::{Activity, TerminalBackend, TerminalController};

pub const STREAM_REDRAW_INTERVAL: Duration = Duration::from_millis(50);

pub struct StreamProgress {
    last_spinner: Instant,
    last_redraw: Instant,
    needs_redraw: bool,
}

impl StreamProgress {
    pub fn new() -> Self {
        Self {
            last_spinner: Instant::now(),
            last_redraw: Instant::now(),
            needs_redraw: false,
        }
    }

    pub fn on_chunk(&mut self) -> bool {
        self.needs_redraw = true;
        if self.last_redraw.elapsed() >= STREAM_REDRAW_INTERVAL {
            self.last_redraw = Instant::now();
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub fn on_tick<B: TerminalBackend>(&mut self, controller: &mut TerminalController<B>) -> bool {
        let spinner_advanced = if self.last_spinner.elapsed() >= SPINNER_FRAME_INTERVAL {
            self.last_spinner = Instant::now();
            controller.advance_spinner();
            !matches!(controller.state().footer().activity, Activity::Idle)
        } else {
            false
        };
        if self.needs_redraw && self.last_redraw.elapsed() >= STREAM_REDRAW_INTERVAL {
            self.needs_redraw = false;
            self.last_redraw = Instant::now();
            true
        } else {
            spinner_advanced
        }
    }
}
