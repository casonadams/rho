//! Line-delimited JSON output for headless runs: one `UiEnvelope` per line on stdout.

use rho_harness_core::locking::lock;
use rho_harness_core::presentation::{StructuredOutputSink, StructuredPresenter, UiEnvelope, UiEvent};
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Writes each event as one JSON line. When the writer fails (for example the reader closed the
/// pipe) the first failure is reported on stderr and later events are dropped.
pub struct NdjsonSink<W: Write + Send> {
    out: Mutex<W>,
    failed: AtomicBool,
}

impl<W: Write + Send> NdjsonSink<W> {
    pub fn new(out: W) -> Self {
        Self {
            out: Mutex::new(out),
            failed: AtomicBool::new(false),
        }
    }

    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    fn record_failure(&self, error: &dyn std::fmt::Display) {
        if !self.failed.swap(true, Ordering::Relaxed) {
            eprintln!("rho: structured output stopped: {error}");
        }
    }
}

impl<W: Write + Send> StructuredOutputSink for NdjsonSink<W> {
    fn emit(&self, event: UiEvent) {
        if self.has_failed() {
            return;
        }
        let line = match serde_json::to_string(&UiEnvelope::new(event)) {
            Ok(line) => line,
            Err(error) => return self.record_failure(&error),
        };
        let mut out = lock(&self.out);
        if let Err(error) = writeln!(out, "{line}").and_then(|()| out.flush()) {
            self.record_failure(&error);
        }
    }

    fn flush(&self) {
        let mut out = lock(&self.out);
        if let Err(error) = out.flush() {
            self.record_failure(&error);
        }
    }
}

pub fn stdout_presenter() -> StructuredPresenter {
    StructuredPresenter::new(std::sync::Arc::new(NdjsonSink::new(std::io::stdout())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Closed {
        attempts: Arc<Mutex<usize>>,
    }

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            *self.attempts.lock().unwrap() += 1;
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn notice(text: &str) -> UiEvent {
        UiEvent::Notice { text: text.to_string() }
    }

    #[test]
    fn each_event_is_one_json_envelope_line() {
        let buffer = Shared::default();
        let sink = NdjsonSink::new(buffer.clone());

        sink.emit(notice("first"));
        sink.emit(notice("second"));
        sink.flush();

        let written = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2);
        for (line, text) in lines.iter().zip(["first", "second"]) {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["text"], text, "{line}");
            assert_eq!(value["event_version"], rho_harness_core::presentation::UI_EVENT_VERSION);
        }
        assert!(!sink.has_failed());
    }

    #[test]
    fn a_closed_pipe_is_recorded_once_and_later_events_are_dropped() {
        let attempts = Arc::new(Mutex::new(0));
        let sink = NdjsonSink::new(Closed {
            attempts: attempts.clone(),
        });

        sink.emit(notice("lost"));
        assert!(sink.has_failed());
        sink.emit(notice("dropped"));
        sink.flush();

        assert_eq!(*attempts.lock().unwrap(), 1, "no writes after the first failure");
    }
}
