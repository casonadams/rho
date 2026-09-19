use super::accumulator::OutputAccumulator;
use crate::process::{StreamingCommand, configure_shell_command};
use crate::tools::types::ToolResult;
use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;

fn setup_command_child(
    base_dir: &Path,
    command_str: &str,
) -> std::result::Result<(StreamingCommand, UnboundedReceiver<String>), ToolResult> {
    let base = Workspace::new(base_dir);
    let cmd = configure_shell_command(command_str, Some(base.root()));
    StreamingCommand::spawn(cmd)
        .map_err(|e| ToolResult::error(format!("Failed to spawn process for command '{command_str}': {e}")))
}

async fn handle_timeout_cleanup<F: FnMut(&str)>(
    mut cmd: StreamingCommand,
    (rx, on_chunk, accumulator): (&mut UnboundedReceiver<String>, &mut F, &mut OutputAccumulator),
    timeout_sec: u64,
) -> ToolResult {
    cmd.cancel().await;
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
        } else if snapshot.truncation.total_lines > super::accumulator::BASH_PRUNE_LINE_THRESHOLD {
            let log_path = snapshot
                .full_output_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "temp log".to_string());
            let lines = snapshot.truncation.total_lines;
            let size = crate::tools::truncate::format_size(snapshot.truncation.total_bytes);
            format!(
                "{}\n\n[Command completed successfully with exit code 0 ({lines} lines, {size}). Full log: {log_path}]",
                snapshot.formatted_text
            )
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

async fn consume_stream_chunks<F: FnMut(&str)>(
    rx: &mut UnboundedReceiver<String>,
    on_chunk: &mut F,
    accumulator: &mut OutputAccumulator,
) {
    while let Some(chunk) = rx.recv().await {
        on_chunk(&chunk);
        accumulator.append(chunk.as_bytes());
    }
}

async fn execute_process_loop<F: FnMut(&str)>(
    (rx, on_chunk, acc): (&mut UnboundedReceiver<String>, &mut F, &mut OutputAccumulator),
    cmd: &mut StreamingCommand,
) -> std::io::Result<std::process::ExitStatus> {
    consume_stream_chunks(rx, on_chunk, acc).await;
    cmd.drain_tasks().await;
    acc.finish();
    cmd.wait().await
}

async fn run_with_timeout<F>(
    (base_dir, args, mut on_chunk): (&Path, &BashArgs, F),
    timeout_sec: u64,
) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    let (mut cmd, mut rx) = match setup_command_child(base_dir, &args.command) {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let mut acc = OutputAccumulator::new();
    let exec = execute_process_loop((&mut rx, &mut on_chunk, &mut acc), &mut cmd);
    match tokio::time::timeout(Duration::from_secs(timeout_sec), exec).await {
        Ok(Ok(status)) => Ok(format_exit_result(status, &acc)),
        Ok(Err(e)) => Ok(ToolResult::error(format!(
            "Failed waiting for command '{}': {e}",
            args.command
        ))),
        Err(_) => Ok(handle_timeout_cleanup(cmd, (&mut rx, &mut on_chunk, &mut acc), timeout_sec).await),
    }
}

pub async fn run_command_streaming<F>(base_dir: &Path, args: &BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    let timeout_sec = args.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_SEC);
    run_with_timeout((base_dir, args, on_chunk), timeout_sec).await
}
