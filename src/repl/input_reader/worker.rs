use std::io;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::InputEvent;
use super::ReadNext;
use super::paused::Control;

const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);

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

pub(super) fn reader_stopped() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "terminal input reader stopped")
}
