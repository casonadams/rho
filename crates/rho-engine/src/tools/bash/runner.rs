use super::accumulator::OutputAccumulator;
use super::shell::resolve_shell_command;
use crate::tools::types::ToolResult;
use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;

struct TaskGuard(Option<tokio::task::JoinHandle<()>>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            h.abort();
        }
    }
}

fn spawn_reader_task<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    mut reader: R,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> TaskGuard {
    TaskGuard(Some(tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 {
                break;
            }
            let s = String::from_utf8_lossy(&buf[..n]).to_string();
            if tx.send(s).is_err() {
                break;
            }
        }
    })))
}

pub async fn run_command_streaming<F>(base_dir: &Path, args: &BashArgs, mut on_chunk: F) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    let timeout_sec = args.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_SEC);

    let mut cmd = resolve_shell_command(&args.command);
    let base = Workspace::new(base_dir);
    cmd.current_dir(base.root());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    cmd.env("CI", "true");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("PAGER", "cat");
    crate::process::isolate_group(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Failed to spawn process for command '{}': {e}",
                args.command
            )));
        }
    };

    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");
    let mut guard = crate::process::ProcessTreeGuard::new(child);

    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut stdout_task = spawn_reader_task(stdout, chunk_tx.clone());
    let mut stderr_task = spawn_reader_task(stderr, chunk_tx);

    let mut accumulator = OutputAccumulator::new();
    let execution_future = async {
        while let Some(chunk) = chunk_rx.recv().await {
            on_chunk(&chunk);
            accumulator.append(chunk.as_bytes());
        }
        if let Some(h) = stdout_task.0.take() {
            let _ = h.await;
        }
        if let Some(h) = stderr_task.0.take() {
            let _ = h.await;
        }
        accumulator.finish();
        guard.wait().await
    };

    let status = match tokio::time::timeout(Duration::from_secs(timeout_sec), execution_future).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            return Ok(ToolResult::error(format!(
                "Failed waiting for command '{}': {e}",
                args.command
            )));
        }
        Err(_) => {
            drop(stdout_task);
            drop(stderr_task);
            guard.kill().await;
            while let Ok(chunk) = chunk_rx.try_recv() {
                on_chunk(&chunk);
                accumulator.append(chunk.as_bytes());
            }
            accumulator.finish();
            let snapshot = accumulator.snapshot();
            let output = snapshot.formatted_text.trim();
            let status_msg = format!("Command timed out after {timeout_sec} seconds");
            let res = if output.is_empty() {
                status_msg
            } else {
                format!("{output}\n\n{status_msg}")
            };
            return Ok(ToolResult::error(res));
        }
    };

    let exit_code = status.code().unwrap_or(-1);
    let snapshot = accumulator.snapshot();
    let output = snapshot.formatted_text.trim();

    if status.success() {
        let res = if output.is_empty() {
            "[Command completed with exit code 0 (no output)]".to_string()
        } else {
            snapshot.formatted_text
        };
        Ok(ToolResult::success(res))
    } else {
        let status_msg = format!("Command exited with code {exit_code}");
        let res = if output.is_empty() {
            status_msg
        } else {
            format!("{output}\n\n{status_msg}")
        };
        Ok(ToolResult::error(res))
    }
}
