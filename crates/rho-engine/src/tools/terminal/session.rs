use super::keys::InputModes;
use super::screen::{SessionHeader, format_screen};
use crate::tools::bash::shell::{cargo_build_vars, shell_argv};
use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;

const QUIET: Duration = Duration::from_millis(150);
const BRIEF_QUIET: Duration = Duration::from_millis(50);
const BRIEF_LIMIT: Duration = Duration::from_millis(300);
const REDRAW_LIMIT: Duration = Duration::from_secs(1);
const EXIT_POLL: Duration = Duration::from_millis(5);
const SCROLLBACK_CAPACITY: usize = 1000;
const MAX_COLS: u16 = 400;
const MAX_ROWS: u16 = 200;

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: NonZeroU16,
    pub rows: NonZeroU16,
}

impl TermSize {
    pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
        let valid = |value: u16, max: u16| NonZeroU16::new(value).filter(|v| v.get() <= max);
        match (valid(cols, MAX_COLS), valid(rows, MAX_ROWS)) {
            (Some(cols), Some(rows)) => Ok(Self { cols, rows }),
            _ => Err(format!(
                "terminal size {cols}x{rows} is out of range (cols 1-{MAX_COLS}, rows 1-{MAX_ROWS})"
            )),
        }
    }

    fn pty(self) -> PtySize {
        PtySize {
            rows: self.rows.get(),
            cols: self.cols.get(),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// How long to wait for a program to react: until its output pauses, or until `until` is on screen
/// once new output has arrived.
pub struct Wait {
    pub limit: Duration,
    pub until: Option<String>,
    quiet: Duration,
    /// How long to wait for a first byte of output before a pause counts.
    patience: Duration,
}

impl Wait {
    pub fn new(limit: Duration, until: Option<String>) -> Self {
        Self {
            limit,
            until,
            quiet: QUIET,
            patience: Duration::ZERO,
        }
    }

    /// For a program that has only just been started and may be slow to print anything.
    pub fn patient(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    /// A short pause for the tty to echo between writes.
    fn brief() -> Self {
        Self {
            quiet: BRIEF_QUIET,
            ..Self::new(BRIEF_LIMIT, None)
        }
    }

    fn window(&self, began: Instant, deadline: Instant, arrived: bool) -> Instant {
        let pause = Instant::now() + self.quiet;
        let end = match (self.until.is_some(), arrived) {
            (true, _) => deadline,
            (false, false) => pause.max(began + self.patience),
            (false, true) => pause,
        };
        end.min(deadline)
    }
}

/// Answers the terminal queries the emulator leaves to its host, as xterm would, so programs that
/// wait for replies (status, cursor position, device attributes, colors) do not stall.
#[derive(Default)]
pub struct Replies(Vec<u8>);

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let reply = match (i1, c, params) {
            (None, 'n', [[5]]) => "\x1b[0n".to_string(),
            (None, 'n', [[6]]) => {
                let (row, col) = screen.cursor_position();
                format!("\x1b[{};{}R", row + 1, col + 1)
            }
            (Some(b'?'), 'n', [[6]]) => {
                let (row, col) = screen.cursor_position();
                format!("\x1b[?{};{}R", row + 1, col + 1)
            }
            (None, 'c', [] | [[0]]) => "\x1b[?1;2c".to_string(),
            (Some(b'>'), 'c', [] | [[0]]) => "\x1b[>0;0;0c".to_string(),
            (Some(b'>'), 'q', [] | [[0]]) => "\x1bP>|rho\x1b\\".to_string(),
            _ => return,
        };
        self.0.extend(reply.into_bytes());
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let reply: &[u8] = match params {
            [b"10", b"?"] => b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
            [b"11", b"?"] => b"\x1b]11;rgb:0000/0000/0000\x1b\\",
            _ => return,
        };
        self.0.extend_from_slice(reply);
    }
}

impl Replies {
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type SharedParser = Arc<Mutex<vt100::Parser<Replies>>>;

/// A program running in a pseudo-terminal, with an emulated screen kept current by a reader thread.
pub struct TerminalSession {
    pub id: u32,
    pub command: String,
    parser: SharedParser,
    writer: SharedWriter,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    output: watch::Receiver<()>,
}

struct Attached {
    parser: SharedParser,
    writer: SharedWriter,
    output: watch::Receiver<()>,
}

fn attach(id: u32, master: &(dyn MasterPty + Send), size: TermSize) -> Result<Attached, String> {
    let pty_error = |e: anyhow::Error| format!("cannot attach to the pseudo-terminal: {e}");
    let reader = master.try_clone_reader().map_err(pty_error)?;
    let writer: SharedWriter = Arc::new(Mutex::new(master.take_writer().map_err(pty_error)?));
    let parser: SharedParser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
        size.rows,
        size.cols,
        SCROLLBACK_CAPACITY,
        Replies::default(),
    )));
    let (notify, output) = watch::channel(());
    let (thread_parser, thread_writer) = (parser.clone(), writer.clone());
    std::thread::Builder::new()
        .name(format!("terminal-{id}"))
        .spawn(move || pump(reader, &thread_parser, &thread_writer, &notify))
        .map_err(|e| format!("cannot start the terminal reader: {e}"))?;
    Ok(Attached { parser, writer, output })
}

impl TerminalSession {
    pub fn spawn(id: u32, command: &str, cwd: &Path, size: TermSize) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(size.pty())
            .map_err(|e| format!("cannot open a pseudo-terminal: {e}"))?;
        let mut child = pair
            .slave
            .spawn_command(shell_command(command, cwd))
            .map_err(|e| format!("cannot start '{command}': {e}"))?;
        drop(pair.slave);
        let attached = match attach(id, &*pair.master, size) {
            Ok(attached) => attached,
            Err(error) => {
                kill_process_groups(&*pair.master, child.process_id());
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            id,
            command: command.to_string(),
            parser: attached.parser,
            writer: attached.writer,
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            output: attached.output,
        })
    }

    /// A receiver that reports output arriving after this call.
    pub fn subscribe(&self) -> watch::Receiver<()> {
        let mut output = self.output.clone();
        output.borrow_and_update();
        output
    }

    /// A receiver that also reports any output that arrived before this call.
    pub fn subscribe_from_start(&self) -> watch::Receiver<()> {
        self.output.clone()
    }

    pub fn input_modes(&self) -> InputModes {
        let parser = lock(&self.parser);
        InputModes {
            application_cursor: parser.screen().application_cursor(),
            bracketed_paste: parser.screen().bracketed_paste(),
        }
    }

    async fn write(&self, bytes: Vec<u8>) -> Result<(), String> {
        let writer = self.writer.clone();
        tokio::task::spawn_blocking(move || {
            let mut writer = lock(&writer);
            writer.write_all(&bytes).and_then(|()| writer.flush())
        })
        .await
        .map_err(|e| format!("terminal write task failed: {e}"))?
        .map_err(|e| format!("cannot write to terminal {}: {e}", self.id))
    }

    /// Writes each segment in turn, pausing briefly between them so the tty can echo one before the
    /// next can flush its queues.
    pub async fn write_segments(&self, segments: Vec<Vec<u8>>) -> Result<(), String> {
        let last = segments.len().saturating_sub(1);
        for (index, bytes) in segments.into_iter().enumerate() {
            let output = self.subscribe();
            self.write(bytes).await?;
            if index < last {
                self.settle(output, &Wait::brief()).await;
            }
        }
        Ok(())
    }

    /// Waits until output pauses for a moment (or `wait.until` shows up in newly arrived output), the
    /// program closes the terminal, or `wait.limit` passes.
    pub async fn settle(&self, mut output: watch::Receiver<()>, wait: &Wait) {
        let began = Instant::now();
        let deadline = began + wait.limit;
        let mut arrived = false;
        while Instant::now() < deadline {
            let window = wait.window(began, deadline, arrived);
            match tokio::time::timeout_at(window, output.changed()).await {
                Ok(Ok(())) => arrived = true,
                Ok(Err(_)) => return self.reap_until(deadline).await,
                Err(_) => return,
            }
            if wait.until.as_deref().is_some_and(|text| self.shows(text)) {
                return;
            }
        }
    }

    /// The program closed the terminal, so it is exiting: give it until `deadline` to be reaped so the
    /// status reports how it ended.
    async fn reap_until(&self, deadline: Instant) {
        while self.is_running() && Instant::now() < deadline {
            tokio::time::sleep(EXIT_POLL).await;
        }
    }

    pub fn shows(&self, text: &str) -> bool {
        lock(&self.parser).screen().contents().contains(text)
    }

    /// Resizes the terminal and waits for the program to redraw.
    pub async fn resize(&self, size: TermSize) -> Result<(), String> {
        lock(&self.parser).screen_mut().set_size(size.rows, size.cols);
        let output = self.subscribe();
        lock(&self.master)
            .resize(size.pty())
            .map_err(|e| format!("cannot resize terminal {}: {e}", self.id))?;
        self.settle(output, &Wait::new(REDRAW_LIMIT, None)).await;
        Ok(())
    }

    /// `running`, `exited N`, or how the program was terminated.
    pub fn status(&self) -> String {
        match lock(&self.child).try_wait() {
            Ok(None) => "running".to_string(),
            Ok(Some(status)) => describe_exit(&status),
            Err(e) => format!("status unknown: {e}"),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(lock(&self.child).try_wait(), Ok(None))
    }

    /// The header line alone, as `list` shows it.
    pub fn summary(&self) -> String {
        let status = self.status();
        let parser = lock(&self.parser);
        self.header(parser.screen(), &status).line()
    }

    /// The header, up to `scrollback` lines that scrolled off the top, and the numbered screen rows.
    pub fn render(&self, scrollback: usize) -> String {
        let status = self.status();
        let mut parser = lock(&self.parser);
        let lines = scrollback_lines(parser.screen_mut(), scrollback);
        let screen = parser.screen();
        let (_, cols) = screen.size();
        let rows: Vec<String> = screen.rows(0, cols.get()).collect();
        format_screen(&self.header(screen, &status), &lines, &rows)
    }

    fn header<'a>(&'a self, screen: &vt100::Screen, status: &'a str) -> SessionHeader<'a> {
        let (rows, cols) = screen.size();
        SessionHeader {
            id: self.id,
            command: &self.command,
            cols: cols.get(),
            rows: rows.get(),
            cursor: screen.cursor_position(),
            status,
        }
    }

    /// Kills the program and everything in its foreground process group, and reaps it. Blocks for up
    /// to a few hundred milliseconds while a program that ignores SIGHUP is given time before SIGKILL.
    pub fn terminate(&self) -> String {
        let mut child = lock(&self.child);
        let master = lock(&self.master);
        let ended = match child.try_wait() {
            Ok(Some(status)) => {
                kill_process_groups(&**master, None);
                Ok(status)
            }
            _ => {
                kill_process_groups(&**master, child.process_id());
                child.kill().and_then(|()| child.wait())
            }
        };
        match ended {
            Ok(status) => describe_exit(&status),
            Err(e) => format!("could not be stopped: {e}"),
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// The last `count` lines (at most what was kept) that scrolled off the top, oldest first. The
/// emulator views at most one screenful of history at a time, so longer requests are read in chunks.
fn scrollback_lines(screen: &mut vt100::Screen, count: usize) -> Vec<String> {
    let (rows, cols) = screen.size();
    let mut offset = count.min(screen.scrollback_len());
    let mut lines = Vec::with_capacity(offset);
    while offset > 0 {
        screen.set_scrollback(offset);
        lines.extend(screen.rows(0, cols.get()).take(offset.min(rows.get().into())));
        offset = offset.saturating_sub(rows.get().into());
    }
    screen.set_scrollback(0);
    lines
}

#[cfg(unix)]
fn kill_process_groups(master: &(dyn MasterPty + Send), child_pid: Option<u32>) {
    let leader = child_pid.and_then(|pid| libc::pid_t::try_from(pid).ok());
    for group in master.process_group_leader().into_iter().chain(leader) {
        if group > 1 {
            // SAFETY: killpg only sends a signal; a group that no longer exists yields ESRCH.
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
    }
}

#[cfg(not(unix))]
fn kill_process_groups(_: &(dyn MasterPty + Send), _: Option<u32>) {}

fn shell_command(command: &str, cwd: &Path) -> CommandBuilder {
    let mut cmd = CommandBuilder::from_argv(shell_argv(command));
    cmd.cwd(cwd);
    for key in cargo_build_vars() {
        cmd.env_remove(key);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("PAGER", "cat");
    cmd.env("GIT_PAGER", "cat");
    cmd
}

fn describe_exit(status: &ExitStatus) -> String {
    match status.signal() {
        Some(signal) => format!("terminated by {signal}"),
        None => format!("exited {}", status.exit_code()),
    }
}

fn pump(mut reader: Box<dyn Read + Send>, parser: &SharedParser, writer: &SharedWriter, notify: &watch::Sender<()>) {
    let mut buf = [0u8; 8192];
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        let replies = {
            let mut parser = lock(parser);
            parser.process(&buf[..n]);
            parser.callbacks_mut().take()
        };
        if !replies.is_empty() && lock(writer).write_all(&replies).is_err() {
            break;
        }
        notify.send_replace(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_must_fit_the_supported_range() {
        assert!(TermSize::new(120, 40).is_ok());
        assert!(TermSize::new(MAX_COLS, MAX_ROWS).is_ok());
        for (cols, rows) in [(0, 40), (120, 0), (MAX_COLS + 1, 40), (120, MAX_ROWS + 1)] {
            let error = TermSize::new(cols, rows).unwrap_err();
            assert!(error.contains("out of range"), "{error}");
        }
    }

    #[test]
    fn terminal_queries_are_answered_in_order() {
        let size = TermSize::new(20, 5).unwrap();
        let mut parser = vt100::Parser::new_with_callbacks(size.rows, size.cols, 0, Replies::default());
        parser.process(b"\x1b[3;5H\x1b[6n\x1b[5n\x1b]11;?\x1b\\\x1b[c\x1b]10;?\x07\x1b[0c\x1b[>c\x1b[>q\x1b[?6n");
        let expected: &[u8] = b"\x1b[3;5R\x1b[0n\x1b]11;rgb:0000/0000/0000\x1b\\\x1b[?1;2c\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b[?1;2c\x1b[>0;0;0c\x1bP>|rho\x1b\\\x1b[?3;5R";
        assert_eq!(parser.callbacks_mut().take(), expected);
        assert!(parser.callbacks_mut().take().is_empty());
    }
}
