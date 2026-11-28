use std::time::{Duration, Instant};

/// Arrivals closer together than this belong to one burst, so they do not count as a gap.
const BURST_SPLIT: Duration = Duration::from_millis(20);
/// A stream whose bursts come faster than this is already smooth and is shown as it arrives.
const SMOOTH_GAP: Duration = Duration::from_millis(32);
/// Longest a burst is spread over, which bounds how far the display can trail the stream.
const MAX_SPREAD: Duration = Duration::from_millis(600);
const FRAME: Duration = Duration::from_millis(16);

/// What a paced stream is released in: whole lines for tool output, words for prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Line,
    Word,
}

impl Unit {
    fn ends(self, ch: char) -> bool {
        match self {
            Self::Line => ch == '\n',
            Self::Word => ch.is_whitespace(),
        }
    }
}

/// Spreads a stream that arrives in bursts over the pause that follows each burst.
#[derive(Debug)]
pub struct Pacer {
    unit: Unit,
    backlog: String,
    units: usize,
    last_arrival: Option<Instant>,
    last_release: Option<Instant>,
    gap: Duration,
    rate: f64,
    credit: f64,
}

impl Pacer {
    pub fn new(unit: Unit) -> Self {
        Self {
            unit,
            backlog: String::new(),
            units: 0,
            last_arrival: None,
            last_release: None,
            gap: Duration::ZERO,
            rate: 0.0,
            credit: 0.0,
        }
    }

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
        let unit = self.unit;
        self.units += chunk.chars().filter(|&c| unit.ends(c)).count();
        self.rate = self.units as f64 / self.gap.min(MAX_SPREAD).as_secs_f64().max(f64::EPSILON);
    }

    /// The part of the backlog to show now: whole units at the pace that drains it by the next burst.
    /// A tail that is not yet a whole unit is shown once nothing else is waiting.
    pub fn release(&mut self, now: Instant) -> String {
        let units = self.units;
        if units == 0 || self.gap < SMOOTH_GAP {
            return self.take_all();
        }
        let elapsed = self
            .last_release
            .map_or(FRAME, |last| now.saturating_duration_since(last))
            .min(MAX_SPREAD);
        self.last_release = Some(now);
        self.credit += self.rate * elapsed.as_secs_f64();
        let due = self.credit.floor() as usize;
        self.credit -= due as f64;
        self.take_units(due.min(units))
    }

    pub fn take_all(&mut self) -> String {
        self.last_release = None;
        self.credit = 0.0;
        self.units = 0;
        std::mem::take(&mut self.backlog)
    }

    pub fn clear(&mut self) {
        self.take_all();
    }

    pub fn is_empty(&self) -> bool {
        self.backlog.is_empty()
    }

    fn take_units(&mut self, count: usize) -> String {
        if count == 0 {
            return String::new();
        }
        let unit = self.unit;
        let end = match self
            .backlog
            .char_indices()
            .filter(|&(_, c)| unit.ends(c))
            .nth(count - 1)
        {
            Some((index, c)) => {
                self.units -= count;
                index + c.len_utf8()
            }
            None => {
                self.units = 0;
                self.backlog.len()
            }
        };
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

    fn drain_over(pacer: &mut Pacer, base: Instant, start_ms: u64) -> (String, Vec<usize>) {
        let mut shown = String::new();
        let mut per_frame = Vec::new();
        for frame in 1..=80u64 {
            let released = pacer.release(at(base, start_ms + frame * 16));
            per_frame.push(released.matches('\n').count());
            shown.push_str(&released);
            if pacer.is_empty() {
                break;
            }
        }
        (shown, per_frame)
    }

    #[test]
    fn a_steady_stream_is_shown_as_it_arrives() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Line);
        for step in 0..10u64 {
            pacer.push(&format!("row {step}\n"), at(base, step * 25));
            assert_eq!(pacer.release(at(base, step * 25)), format!("row {step}\n"));
        }
        assert!(pacer.is_empty());
    }

    #[test]
    fn a_burst_after_a_pause_is_spread_over_the_pause_in_order() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Line);
        pacer.push(&lines(1..=1), at(base, 0));
        pacer.release(at(base, 0));
        pacer.push(&lines(2..=21), at(base, 400));

        let (shown, per_frame) = drain_over(&mut pacer, base, 400);

        assert_eq!(shown, lines(2..=21));
        assert!(per_frame.iter().all(|&n| n <= 2), "{per_frame:?}");
        assert!((20..=30).contains(&per_frame.len()), "{} frames", per_frame.len());
    }

    #[test]
    fn a_small_burst_is_released_one_unit_at_a_time_not_all_at_once() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Line);
        pacer.push(&lines(1..=1), at(base, 0));
        pacer.release(at(base, 0));
        pacer.push(&lines(2..=4), at(base, 400));

        let (shown, per_frame) = drain_over(&mut pacer, base, 400);

        assert_eq!(shown, lines(2..=4));
        let showing_frames = per_frame.iter().filter(|&&n| n > 0).count();
        assert_eq!(showing_frames, 3, "{per_frame:?}");
        assert!(per_frame.len() >= 20, "3 lines spread over {} frames", per_frame.len());
    }

    #[test]
    fn prose_is_released_a_few_words_at_a_time_and_never_splits_a_word() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Word);
        pacer.push("Hello there ", at(base, 0));
        pacer.release(at(base, 0));
        let sentence = "the quick brown fox jumps over the lazy dog and keeps running far away ";
        pacer.push(sentence, at(base, 400));

        let mut shown = String::new();
        for frame in 1..=60u64 {
            let released = pacer.release(at(base, 400 + frame * 16));
            assert!(released.is_empty() || released.ends_with(' '), "{released:?}");
            assert!(released.split_whitespace().count() <= 3, "{released:?}");
            shown.push_str(&released);
            if pacer.is_empty() {
                break;
            }
        }
        assert_eq!(shown, sentence);
    }

    #[test]
    fn a_trailing_partial_unit_waits_only_for_the_units_before_it() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Line);
        pacer.push("a\n", at(base, 0));
        pacer.push("b\n", at(base, 300));
        pacer.release(at(base, 316));
        pacer.push("partial", at(base, 320));
        let (shown, _) = drain_over(&mut pacer, base, 320);
        assert_eq!(shown, "a\nb\npartial");
    }

    #[test]
    fn the_unit_count_follows_the_backlog_across_pushes_and_releases() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Word);
        let recount = |pacer: &Pacer| pacer.backlog.chars().filter(|c| c.is_whitespace()).count();
        pacer.push("one two ", at(base, 0));
        pacer.release(at(base, 0));
        let steps: [(&str, u64); 4] = [
            ("three four five ", 400),
            ("six", 420),
            (" seven\neight ", 800),
            ("", 820),
        ];
        for (chunk, ms) in steps {
            pacer.push(chunk, at(base, ms));
            assert_eq!(pacer.units, recount(&pacer), "after pushing {chunk:?}");
            for frame in 1..=3 {
                pacer.release(at(base, ms + frame * 16));
                assert_eq!(pacer.units, recount(&pacer), "after a release at {ms}ms");
            }
        }
        pacer.take_all();
        assert_eq!(pacer.units, 0);
    }

    #[test]
    fn take_all_returns_everything_in_order_and_empties_the_backlog() {
        let base = Instant::now();
        let mut pacer = Pacer::new(Unit::Line);
        pacer.push(&lines(1..=1), at(base, 0));
        pacer.push(&lines(2..=30), at(base, 500));
        assert_eq!(pacer.take_all(), lines(1..=30));
        assert!(pacer.is_empty());
    }
}
