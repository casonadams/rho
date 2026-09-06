use super::accumulator::OutputAccumulator;
use super::shell::resolve_shell_command;
use crate::tools::types::ToolResult;
use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

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

fn configure_child_command(base_dir: &Path, command_str: &str) -> Command {
    let mut cmd = resolve_shell_command(command_str);
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
    cmd
}

fn setup_command_child(
    base_dir: &Path,
    command_str: &str,
) -> std::result::Result<
    (
        crate::process::ProcessTreeGuard,
        TaskGuard,
        TaskGuard,
        tokio::sync::mpsc::UnboundedReceiver<String>,
    ),
    ToolResult,
> {
    let mut cmd = configure_child_command(base_dir, command_str);
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolResult::error(format!("Failed to spawn process for command '{command_str}': {e}")))?;
    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");
    let guard = crate::process::ProcessTreeGuard::new(child);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let stdout_task = spawn_reader_task(stdout, tx.clone());
    let stderr_task = spawn_reader_task(stderr, tx);
    Ok((guard, stdout_task, stderr_task, rx))
}

async fn handle_timeout_cleanup<F: FnMut(&str)>(
    (mut guard, stdout_task, stderr_task): (crate::process::ProcessTreeGuard, TaskGuard, TaskGuard),
    (rx, on_chunk, accumulator): (
        &mut tokio::sync::mpsc::UnboundedReceiver<String>,
        &mut F,
        &mut OutputAccumulator,
    ),
    timeout_sec: u64,
) -> ToolResult {
    drop(stdout_task);
    drop(stderr_task);
    guard.kill().await;
    while let Ok(chunk) = rx.try_recv() {
        on_chunk(&chunk);
        accumulator.append(chunk.as_bytes());
    }
    accumulator.finish();
    let snapshot = accumulator.snapshot();
    let output = snapshot.formatted_text.trim();
    let msg = format!("Command timed out after {timeout_sec} seconds");
    let res = if output.is_empty() {
        msg
    } else {
        format!("{output}\n\n{msg}")
    };
    ToolResult::error(res)
}

fn format_exit_result(status: std::process::ExitStatus, accumulator: &OutputAccumulator) -> ToolResult {
    let snapshot = accumulator.snapshot();
    let output = snapshot.formatted_text.trim();
    if status.success() {
        let res = if output.is_empty() {
            "[Command completed with exit code 0 (no output)]".to_string()
        } else {
            snapshot.formatted_text
        };
        ToolResult::success(res)
    } else {
        let exit_code = status.code().unwrap_or(-1);
        let msg = format!("Command exited with code {exit_code}");
        let res = if output.is_empty() {
            msg
        } else {
            format!("{output}\n\n{msg}")
        };
        ToolResult::error(res)
    }
}

async fn await_task_pair(stdout_task: &mut TaskGuard, stderr_task: &mut TaskGuard) {
    if let Some(h) = stdout_task.0.take() {
        let _ = h.await;
    }
    if let Some(h) = stderr_task.0.take() {
        let _ = h.await;
    }
}

async fn consume_stream_chunks<F: FnMut(&str)>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    on_chunk: &mut F,
    accumulator: &mut OutputAccumulator,
) {
    while let Some(chunk) = rx.recv().await {
        on_chunk(&chunk);
        accumulator.append(chunk.as_bytes());
    }
}

async fn execute_process_loop<F: FnMut(&str)>(
    (rx, on_chunk, acc): (
        &mut tokio::sync::mpsc::UnboundedReceiver<String>,
        &mut F,
        &mut OutputAccumulator,
    ),
    (stdout_task, stderr_task, guard): (&mut TaskGuard, &mut TaskGuard, &mut crate::process::ProcessTreeGuard),
) -> std::io::Result<std::process::ExitStatus> {
    consume_stream_chunks(rx, on_chunk, acc).await;
    await_task_pair(stdout_task, stderr_task).await;
    acc.finish();
    guard.wait().await
}

async fn run_with_timeout<F>(
    (base_dir, args, mut on_chunk): (&Path, &BashArgs, F),
    timeout_sec: u64,
) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    let (mut guard, mut stdout_task, mut stderr_task, mut rx) = match setup_command_child(base_dir, &args.command) {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let mut acc = OutputAccumulator::new();
    let exec = execute_process_loop(
        (&mut rx, &mut on_chunk, &mut acc),
        (&mut stdout_task, &mut stderr_task, &mut guard),
    );
    match tokio::time::timeout(Duration::from_secs(timeout_sec), exec).await {
        Ok(Ok(status)) => Ok(format_exit_result(status, &acc)),
        Ok(Err(e)) => Ok(ToolResult::error(format!(
            "Failed waiting for command '{}': {e}",
            args.command
        ))),
        Err(_) => Ok(handle_timeout_cleanup(
            (guard, stdout_task, stderr_task),
            (&mut rx, &mut on_chunk, &mut acc),
            timeout_sec,
        )
        .await),
    }
}

pub async fn run_command_streaming<F>(base_dir: &Path, args: &BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    let timeout_sec = args.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_SEC);
    run_with_timeout((base_dir, args, on_chunk), timeout_sec).await
}
