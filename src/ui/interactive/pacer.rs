use std::time::{Duration, Instant};

/// Arrivals closer together than this belong to one burst, so they do not count as a gap.
const BURST_SPLIT: Duration = Duration::from_millis(20);
/// A stream whose bursts come faster than this is already smooth and is shown as it arrives.
const SMOOTH_GAP: Duration = Duration::from_millis(32);
/// Longest a burst is spread over, which bounds how far the display can trail the stream.
const MAX_SPREAD: Duration = Duration::from_millis(600);
const FRAME: Duration = Duration::from_millis(16);

/// Spreads tool output that arrives in bursts over the time until the next burst, a few lines per frame.
#[derive(Debug, Default)]
pub struct ChunkPacer {
    backlog: String,
    last_arrival: Option<Instant>,
    last_release: Option<Instant>,
    gap: Duration,
}

impl ChunkPacer {
    pub fn push(&mut self, chunk: &str, now: Instant) {
        if let Some(previous) = self.last_arrival {
            let observed = now.saturating_duration_since(previous);
            if observed >= BURST_SPLIT {
                let observed = observed.min(MAX_SPREAD * 2);
                self.gap = if self.gap.is_zero() {
                    observed
                } else {
                    self.gap.mul_f32(0.7) + observed.mul_f32(0.3)
                };
            }
        }
        self.last_arrival = Some(now);
        self.backlog.push_str(chunk);
    }

    /// The part of the backlog to show now. A trailing partial line waits only for the lines before it.
    pub fn release(&mut self, now: Instant) -> String {
        let lines = self.backlog.matches('\n').count();
        if lines == 0 || self.gap < SMOOTH_GAP {
            return self.take_all();
        }
        let spread = self.gap.min(MAX_SPREAD);
        let elapsed = self
            .last_release
            .map_or(FRAME, |last| now.saturating_duration_since(last))
            .min(spread);
        self.last_release = Some(now);
        let due = (lines as f64 * elapsed.as_secs_f64() / spread.as_secs_f64()).ceil() as usize;
        self.take_lines(due.clamp(1, lines))
    }

    pub fn take_all(&mut self) -> String {
        self.last_release = None;
        std::mem::take(&mut self.backlog)
    }

    pub fn clear(&mut self) {
        self.take_all();
    }

    pub fn is_empty(&self) -> bool {
        self.backlog.is_empty()
    }

    fn take_lines(&mut self, count: usize) -> String {
        let end = self
            .backlog
            .match_indices('\n')
            .nth(count - 1)
            .map_or(self.backlog.len(), |(index, _)| index + 1);
        let rest = self.backlog.split_off(end);
        std::mem::replace(&mut self.backlog, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn lines(range: std::ops::RangeInclusive<usize>) -> String {
        range.map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn a_steady_stream_is_shown_as_it_arrives() {
        let base = Instant::now();
        let mut pacer = ChunkPacer::default();
        for step in 0..10u64 {
            pacer.push(&format!("row {step}\n"), at(base, step * 25));
            assert_eq!(pacer.release(at(base, step * 25)), format!("row {step}\n"));
        }
        assert!(pacer.is_empty());
    }

    #[test]
    fn a_burst_after_a_pause_is_spread_over_the_pause_in_order() {
        let base = Instant::now();
        let mut pacer = ChunkPacer::default();
        pacer.push(&lines(1..=1), at(base, 0));
        pacer.release(at(base, 0));
        pacer.push(&lines(2..=21), at(base, 400));

        let mut shown = String::new();
        let mut frames = 0;
        let mut first_batch = None;
        for frame in 1..=40u64 {
            let released = pacer.release(at(base, 400 + frame * 16));
            first_batch.get_or_insert_with(|| released.lines().count());
            shown.push_str(&released);
            frames = frame;
            if pacer.is_empty() {
                break;
            }
        }

        assert_eq!(shown, lines(2..=21));
        let first = first_batch.unwrap();
        assert!((1..=2).contains(&first), "first frame showed {first} of 20 lines");
        assert!((20..=40).contains(&frames), "took {frames} frames to drain");
    }

    #[test]
    fn a_partial_line_waits_only_for_the_lines_before_it() {
        let base = Instant::now();
        let mut pacer = ChunkPacer::default();
        pacer.push("a\n", at(base, 0));
        pacer.push("b\n", at(base, 300));
        pacer.release(at(base, 316));
        pacer.push("partial", at(base, 320));
        assert_eq!(pacer.release(at(base, 336)), "b\n");
        assert_eq!(pacer.release(at(base, 352)), "partial");
        assert!(pacer.is_empty());
    }

    #[test]
    fn take_all_returns_everything_in_order_and_empties_the_backlog() {
        let base = Instant::now();
        let mut pacer = ChunkPacer::default();
        pacer.push(&lines(1..=1), at(base, 0));
        pacer.push(&lines(2..=30), at(base, 500));
        assert_eq!(pacer.take_all(), lines(1..=30));
        assert!(pacer.is_empty());
    }
}
