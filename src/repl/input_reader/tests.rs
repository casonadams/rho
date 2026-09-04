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
