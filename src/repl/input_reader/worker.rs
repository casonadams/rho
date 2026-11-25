use std::io;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::InputEvent;
use super::ReadNext;
use super::paused::Control;

const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// A width change re-renders the whole transcript, so a burst of resize events (dragging the
/// window) is delivered as its final size once no further resize arrives for this long.
const RESIZE_SETTLE: Duration = Duration::from_millis(15);

type Size = (u16, u16);

fn settle_resize(read_next: &mut ReadNext, mut size: Size) -> (Size, io::Result<Option<crossterm::event::Event>>) {
    loop {
        match read_next(RESIZE_SETTLE) {
            Ok(Some(crossterm::event::Event::Resize(cols, rows))) => size = (cols, rows),
            other => return (size, other),
        }
    }
}

/// Returns false once the receiving side is gone or the source failed, which ends the read loop.
fn deliver(
    event_sender: &mpsc::UnboundedSender<InputEvent>,
    read: io::Result<Option<crossterm::event::Event>>,
) -> bool {
    match read {
        Ok(Some(crossterm::event::Event::Key(key))) if key.kind == crossterm::event::KeyEventKind::Release => true,
        Ok(Some(event)) => event_sender.send(Ok(event)).is_ok(),
        Ok(None) => true,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => true,
        Err(error) => {
            let _ = event_sender.send(Err(error));
            false
        }
    }
}

pub(super) fn read_loop(
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

        let keep_going = match read_next(CONTROL_POLL_INTERVAL) {
            Ok(Some(crossterm::event::Event::Resize(cols, rows))) => {
                let (size, follow_up) = settle_resize(&mut read_next, (cols, rows));
                let resize = Ok(Some(crossterm::event::Event::Resize(size.0, size.1)));
                deliver(&event_sender, resize) && deliver(&event_sender, follow_up)
            }
            other => deliver(&event_sender, other),
        };
        if !keep_going {
            return;
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

pub(super) fn reader_stopped() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "terminal input reader stopped")
}
