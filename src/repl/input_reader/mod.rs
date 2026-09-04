use std::{
    io,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossterm::event::{self, Event};
use tokio::sync::mpsc;

mod paused;
mod worker;

#[cfg(test)]
mod tests;

use paused::Control;
pub(crate) use paused::PausedInput;
use worker::{read_loop, reader_stopped};

const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(1);

type ReadNext = Box<dyn FnMut(Duration) -> io::Result<Option<Event>> + Send>;

pub(super) type InputEvent = io::Result<Event>;

pub(crate) struct TerminalInputReader {
    events: mpsc::UnboundedReceiver<InputEvent>,
    control: std_mpsc::Sender<Control>,
    thread: Option<JoinHandle<()>>,
}

impl TerminalInputReader {
    pub(crate) fn spawn() -> io::Result<Self> {
        Self::spawn_with(Box::new(|timeout| {
            if event::poll(timeout)? {
                event::read().map(Some)
            } else {
                Ok(None)
            }
        }))
    }

    #[cfg(test)]
    pub(crate) fn spawn_dummy() -> Self {
        Self::spawn_with(Box::new(|_| Ok(None))).expect("spawn dummy reader")
    }

    #[cfg(test)]
    pub(crate) fn spawn_with_events(events: Vec<crossterm::event::Event>) -> Self {
        let events = std::sync::Mutex::new(events.into_iter());
        Self::spawn_with(Box::new(move |timeout| {
            if let Ok(mut iter) = events.lock()
                && let Some(evt) = iter.next()
            {
                return Ok(Some(evt));
            }
            std::thread::sleep(timeout);
            Ok(None)
        }))
        .expect("spawn mock reader")
    }

    fn spawn_with(read_next: ReadNext) -> io::Result<Self> {
        let (event_sender, events) = mpsc::unbounded_channel();
        let (control, controls) = std_mpsc::channel();
        let thread = thread::Builder::new()
            .name("rho-terminal-input".to_string())
            .spawn(move || read_loop(read_next, event_sender, controls))?;
        Ok(Self {
            events,
            control,
            thread: Some(thread),
        })
    }

    pub(crate) async fn recv(&mut self) -> Option<InputEvent> {
        self.events.recv().await
    }

    pub(crate) fn pause(&self) -> io::Result<PausedInput<'_>> {
        let (acknowledge, acknowledged) = std_mpsc::sync_channel(1);
        self.control
            .send(Control::Pause(acknowledge))
            .map_err(|_| reader_stopped())?;
        acknowledged
            .recv_timeout(CONTROL_ACK_TIMEOUT)
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "terminal input reader did not pause"))?;
        Ok(PausedInput {
            reader: self,
            resumed: false,
        })
    }

    pub(crate) fn stop_and_join(&mut self) -> io::Result<()> {
        let _ = self.control.send(Control::Stop);
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        thread
            .join()
            .map_err(|_| io::Error::other("terminal input reader thread panicked"))
    }
}

impl Drop for TerminalInputReader {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}
