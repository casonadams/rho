use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use rho_engine::process::{ProcessTreeGuard, isolate_group};

pub(super) struct RunningCommand {
    pub guard: ProcessTreeGuard,
    pub stdout_task: JoinHandle<()>,
    pub stderr_task: JoinHandle<()>,
}

impl RunningCommand {
    pub(super) fn spawn(cmd: &str) -> std::io::Result<(Self, UnboundedReceiver<String>)> {
        let mut command = configure_shell_command(cmd);
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let guard = ProcessTreeGuard::new(child);
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::unbounded_channel();
        let stdout_task = spawn_stream_reader(stdout, chunk_tx.clone());
        let stderr_task = spawn_stream_reader(stderr, chunk_tx);
        Ok((
            Self {
                guard,
                stdout_task,
                stderr_task,
            },
            chunk_rx,
        ))
    }

    pub(super) async fn cancel(&mut self) {
        self.stdout_task.abort();
        self.stderr_task.abort();
        self.guard.kill().await;
    }

    pub(super) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.guard.wait().await
    }

    pub(super) async fn drain_tasks(&mut self) {
        let _ = (&mut self.stdout_task).await;
        let _ = (&mut self.stderr_task).await;
    }
}

fn configure_shell_command(cmd: &str) -> tokio::process::Command {
    let mut command = rho_engine::tools::bash::resolve_shell_command(cmd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);
    command.env("CI", "true");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("PAGER", "cat");
    isolate_group(&mut command);
    command
}

fn spawn_stream_reader<R: AsyncReadExt + Unpin + Send + 'static>(
    mut reader: R,
    tx: UnboundedSender<String>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 || tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).is_err() {
                break;
            }
        }
    })
}
