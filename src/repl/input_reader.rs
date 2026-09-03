use std::{
    io,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossterm::event::{self, Event};
use tokio::sync::mpsc;

const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(1);

type ReadNext = Box<dyn FnMut(Duration) -> io::Result<Option<Event>> + Send>;

pub(super) type InputEvent = io::Result<Event>;

enum Control {
    Pause(std_mpsc::SyncSender<()>),
    Resume,
    Stop,
}

pub(super) struct TerminalInputReader {
    events: mpsc::UnboundedReceiver<InputEvent>,
    control: std_mpsc::Sender<Control>,
    thread: Option<JoinHandle<()>>,
}

impl TerminalInputReader {
    pub(super) fn spawn() -> io::Result<Self> {
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

    pub(super) async fn recv(&mut self) -> Option<InputEvent> {
        self.events.recv().await
    }

    pub(super) fn pause(&self) -> io::Result<PausedInput<'_>> {
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

    pub(super) fn stop_and_join(&mut self) -> io::Result<()> {
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

pub(super) struct PausedInput<'a> {
    reader: &'a TerminalInputReader,
    resumed: bool,
}

impl PausedInput<'_> {
    pub(super) fn resume(mut self) -> io::Result<()> {
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

fn read_loop(
    mut read_next: ReadNext,
    event_sender: mpsc::UnboundedSender<InputEvent>,
    controls: std_mpsc::Receiver<Control>,
) {
    loop {
        match controls.try_recv() {
            Ok(Control::Pause(acknowledge)) => {
                let _ = acknowledge.send(());
                if !wait_until_resumed(&controls) {
                    return;
                }
            }
            Ok(Control::Resume) | Err(std_mpsc::TryRecvError::Empty) => {}
            Ok(Control::Stop) | Err(std_mpsc::TryRecvError::Disconnected) => return,
        }

        match read_next(CONTROL_POLL_INTERVAL) {
            Ok(Some(event)) => {
                if event_sender.send(Ok(event)).is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                let _ = event_sender.send(Err(error));
                return;
            }
        }
    }
}

fn wait_until_resumed(controls: &std_mpsc::Receiver<Control>) -> bool {
    loop {
        match controls.recv() {
            Ok(Control::Resume) => return true,
            Ok(Control::Pause(acknowledge)) => {
                let _ = acknowledge.send(());
            }
            Ok(Control::Stop) | Err(_) => return false,
        }
    }
}

fn reader_stopped() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "terminal input reader stopped")
}

#[cfg(test)]
mod tests {
    use std::{io, sync::mpsc as std_mpsc, time::Duration};

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::TerminalInputReader;

    enum SourceCommand {
        Event(Event),
        Error,
    }

    fn test_reader() -> (
        TerminalInputReader,
        std_mpsc::Sender<SourceCommand>,
        std_mpsc::Receiver<()>,
    ) {
        let (sender, receiver) = std_mpsc::channel();
        let (read_started, reads) = std_mpsc::channel();
        let reader = TerminalInputReader::spawn_with(Box::new(move |timeout| {
            let _ = read_started.send(());
            match receiver.recv_timeout(timeout) {
                Ok(SourceCommand::Event(event)) => Ok(Some(event)),
                Ok(SourceCommand::Error) => Err(io::Error::other("input failed")),
                Err(std_mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(std_mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other("source closed")),
            }
        }))
        .unwrap();
        (reader, sender, reads)
    }

    #[tokio::test]
    async fn forwards_events_and_propagates_input_errors() {
        let (mut reader, source, _) = test_reader();
        let event = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        source.send(SourceCommand::Event(event.clone())).unwrap();
        source.send(SourceCommand::Error).unwrap();

        assert_eq!(reader.recv().await.unwrap().unwrap(), event);
        assert_eq!(reader.recv().await.unwrap().unwrap_err().kind(), io::ErrorKind::Other);
        assert!(reader.recv().await.is_none());
        reader.stop_and_join().unwrap();
    }

    #[test]
    fn pause_is_acknowledged_and_prevents_reads_until_resume() {
        let (mut reader, _source, reads) = test_reader();
        reads.recv_timeout(Duration::from_secs(1)).unwrap();

        let paused = reader.pause().unwrap();
        while reads.try_recv().is_ok() {}
        assert!(matches!(
            reads.recv_timeout(Duration::from_millis(40)),
            Err(std_mpsc::RecvTimeoutError::Timeout)
        ));

        paused.resume().unwrap();
        reads.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.stop_and_join().unwrap();
    }

    #[test]
    fn shutdown_stops_and_joins_a_paused_reader() {
        let (mut reader, _source, _) = test_reader();
        let paused = reader.pause().unwrap();
        drop(paused);
        reader.stop_and_join().unwrap();
        reader.stop_and_join().unwrap();
    }
}
