//! Applies a terminal size noticed on a frame tick only once it has held still, so dragging a
//! window edge reflows the transcript once instead of on every frame.

use crate::ui::interactive::{TerminalBackend, TerminalController};
use std::io;
use std::time::Duration;
use tokio::time::Instant;

pub const RESIZE_SETTLE: Duration = Duration::from_millis(100);

type Size = (usize, usize);

#[derive(Debug, Default)]
pub struct SizeSettle {
    pending: Option<(Size, Instant)>,
}

impl SizeSettle {
    /// Whether `reported`, seen at `now` while `shown` is applied, has held still long enough.
    fn settled(&mut self, shown: Size, reported: Size, now: Instant) -> bool {
        if reported == shown {
            self.pending = None;
            return false;
        }
        match self.pending {
            Some((size, since)) if size == reported => {
                let ready = now.duration_since(since) >= RESIZE_SETTLE;
                if ready {
                    self.pending = None;
                }
                ready
            }
            _ => {
                self.pending = Some((reported, now));
                false
            }
        }
    }

    /// Whether a size change is waiting to settle.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Applies the terminal's size once it has stayed the same for `RESIZE_SETTLE`; returns whether
    /// the size changed.
    pub fn refresh<B: TerminalBackend>(&mut self, controller: &mut TerminalController<B>) -> io::Result<bool> {
        let reported = controller.probe_size()?;
        let shown = (controller.width(), controller.height());
        if !self.settled(shown, reported, Instant::now()) {
            return Ok(false);
        }
        controller.refresh_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::controller::tests::fake::controller_with_size;
    use test_case::test_case;

    #[test_case(&[((80, 24), 0), ((80, 24), 200)], false; "unchanged")]
    #[test_case(&[((60, 24), 0)], false; "first sighting")]
    #[test_case(&[((60, 24), 0), ((60, 24), 99)], false; "still settling")]
    #[test_case(&[((60, 24), 0), ((60, 24), 100)], true; "settled")]
    #[test_case(&[((60, 24), 0), ((50, 24), 100)], false; "moved again")]
    #[test_case(&[((60, 24), 0), ((50, 24), 50), ((50, 24), 150)], true; "settled after moving")]
    #[test_case(&[((60, 24), 0), ((80, 24), 50), ((60, 24), 120)], false; "moved back")]
    fn a_size_applies_only_after_it_holds_still(steps: &[(Size, u64)], expected: bool) {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let shown = (80, 24);
        let mut settle = SizeSettle::default();
        let last = steps
            .iter()
            .map(|&(size, ms)| settle.settled(shown, size, at(ms)))
            .last()
            .unwrap();
        assert_eq!(last, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_applies_the_reported_size_once_settled() {
        let (mut controller, _ops, width, _height) = controller_with_size(80, 24);
        let mut settle = SizeSettle::default();
        width.set(60);

        assert!(!settle.refresh(&mut controller).unwrap());
        assert!(settle.is_pending());
        tokio::time::advance(RESIZE_SETTLE / 2).await;
        assert!(!settle.refresh(&mut controller).unwrap());
        assert_eq!(controller.width(), 80);

        tokio::time::advance(RESIZE_SETTLE).await;
        assert!(settle.refresh(&mut controller).unwrap());
        assert_eq!(controller.width(), 60);
        assert!(!settle.is_pending());
    }
}
