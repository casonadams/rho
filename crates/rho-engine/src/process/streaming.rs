use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use super::{ProcessTreeGuard, isolate_group};

fn handle_utf8_slice(data: &[u8], leftover: &mut Vec<u8>, tx: &UnboundedSender<String>) -> bool {
    match std::str::from_utf8(data) {
        Ok(valid) => {
            leftover.clear();
            tx.send(valid.to_string()).is_ok()
        }
        Err(err) => {
            let valid_up_to = err.valid_up_to();
            if valid_up_to > 0
                && tx
                    .send(String::from_utf8_lossy(&data[..valid_up_to]).to_string())
                    .is_err()
            {
                return false;
            }
            if let Some(error_len) = err.error_len() {
                let start = valid_up_to + error_len;
                leftover.clear();
                leftover.extend_from_slice(&data[start..]);
                tx.send("\u{FFFD}".to_string()).is_ok()
            } else {
                *leftover = data[valid_up_to..].to_vec();
                true
            }
        }
    }
}

fn process_read_buffer(buf: &[u8], leftover: &mut Vec<u8>, tx: &UnboundedSender<String>) -> bool {
    if leftover.is_empty() {
        handle_utf8_slice(buf, leftover, tx)
    } else {
        leftover.extend_from_slice(buf);
        let data = std::mem::take(leftover);
        handle_utf8_slice(&data, leftover, tx)
    }
}

fn spawn_reader_task<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    mut reader: R,
    tx: UnboundedSender<String>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut leftover = Vec::new();
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 || !process_read_buffer(&buf[..n], &mut leftover, &tx) {
                break;
            }
        }
        if !leftover.is_empty() {
            let s = String::from_utf8_lossy(&leftover).to_string();
            let _ = tx.send(s);
        }
    })
}

pub fn configure_shell_command(cmd_str: &str, working_dir: Option<&Path>) -> Command {
    let mut command = crate::tools::bash::resolve_shell_command(cmd_str);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);
    command.env("CI", "true");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("PAGER", "cat");
    command.env("GIT_PAGER", "cat");
    command.env("CLICOLOR", "1");
    command.env("CLICOLOR_FORCE", "1");
    command.env("FORCE_COLOR", "1");
    command.env("COLORTERM", "truecolor");
    command.env("TERM", "xterm-256color");
    isolate_group(&mut command);
    command
}

pub struct StreamingCommand {
    guard: ProcessTreeGuard,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
}

impl StreamingCommand {
    pub fn spawn(mut command: Command) -> std::io::Result<(Self, UnboundedReceiver<String>)> {
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let guard = ProcessTreeGuard::new(child);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stdout_task = spawn_reader_task(stdout, tx.clone());
        let stderr_task = spawn_reader_task(stderr, tx);
        Ok((
            Self {
                guard,
                stdout_task,
                stderr_task,
            },
            rx,
        ))
    }

    pub async fn cancel(&mut self) {
        self.stdout_task.abort();
        self.stderr_task.abort();
        self.guard.kill().await;
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.guard.wait().await
    }

    pub async fn drain_tasks(&mut self) {
        let _ = (&mut self.stdout_task).await;
        let _ = (&mut self.stderr_task).await;
    }

    pub fn guard(&self) -> &ProcessTreeGuard {
        &self.guard
    }

    pub fn guard_mut(&mut self) -> &mut ProcessTreeGuard {
        &mut self.guard
    }
}
