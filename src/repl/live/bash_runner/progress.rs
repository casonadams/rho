use std::time::{Duration, Instant};

use super::super::batch::SPINNER_FRAME_INTERVALS;
use crate::ui::interactive::{Activity, TerminalBackend, TerminalController};

const STREAM_REDRAW_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct StreamProgress {
    spinner_tick: usize,
    last_redraw: Instant,
    needs_redraw: bool,
}

impl StreamProgress {
    pub(super) fn new() -> Self {
        Self {
            spinner_tick: 0,
            last_redraw: Instant::now(),
            needs_redraw: false,
        }
    }

    pub(super) fn on_chunk(&mut self) -> bool {
        self.needs_redraw = true;
        if self.last_redraw.elapsed() >= STREAM_REDRAW_INTERVAL {
            self.last_redraw = Instant::now();
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub(super) fn on_tick<B: TerminalBackend>(&mut self, controller: &mut TerminalController<B>) -> bool {
        self.spinner_tick += 1;
        let spinner_advanced = if self.spinner_tick >= SPINNER_FRAME_INTERVALS {
            self.spinner_tick = 0;
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
