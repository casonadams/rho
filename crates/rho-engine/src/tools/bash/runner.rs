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

pub fn format_background_notice(job_id: &str) -> String {
    format!(
        "Backgrounded as job '{job_id}'; its output is injected into the conversation as a follow-up the moment it finishes. Do NOT poll for it (no `sleep`, `ps`, `pgrep`, `top`): every poll is a wasted turn. Continue other work, call `wait` if blocked, or end your reply and wait to be woken."
    )
}

fn dispatch_background_task(
    mut cmd: StreamingCommand,
    mut rx: UnboundedReceiver<String>,
    mut acc: OutputAccumulator,
    job_id: String,
) {
    tokio::spawn(async move {
        let mut chunk_handler = |_chunk: &str| {};
        let status = execute_process_loop(&mut rx, &mut chunk_handler, &mut acc, &mut cmd).await;
        match status {
            Ok(exit_status) => {
                let snapshot = acc.snapshot();
                let output = snapshot.formatted_text.trim().to_string();
                let exit_code = exit_status.code().unwrap_or(-1);
                crate::tools::wait::JobManager::global().update_status(
                    &job_id,
                    crate::tools::wait::JobStatus::Completed {
                        exit_code,
                        output: output.clone(),
                    },
                );
                crate::engine::runner::turn::types::enqueue_background_notification(format!(
                    "<system-notice>\nBackground job '{job_id}' completed with exit code {exit_code}.\nOutput:\n{output}\nResume your work using the results above.\n</system-notice>"
                ));
            }
            Err(e) => {
                crate::tools::wait::JobManager::global()
                    .update_status(&job_id, crate::tools::wait::JobStatus::Failed { error: e.to_string() });
                crate::engine::runner::turn::types::enqueue_background_notification(format!(
                    "<system-notice>\nBackground job '{job_id}' failed: {e}.\nResume your work using the results above.\n</system-notice>"
                ));
            }
        }
    });
}

fn format_exit_result(status: std::process::ExitStatus, accumulator: &OutputAccumulator) -> ToolResult {
    let snapshot = accumulator.snapshot();
    let raw_text = snapshot.formatted_text.trim();
    if status.success() {
        if raw_text.is_empty() {
            return ToolResult::success("[Command completed with exit code 0 (no output)]");
        }
        let collapsed = super::normalize::collapse_consecutive_lines(raw_text);
        let body = if snapshot.truncation.total_lines > super::normalize::BASH_CLEAN_WINDOW_LINE_THRESHOLD {
            super::normalize::window_clean_output(
                &collapsed,
                super::normalize::BASH_CLEAN_HEAD_LINES,
                super::normalize::BASH_CLEAN_TAIL_LINES,
            )
        } else {
            collapsed
        };

        let res = if snapshot.truncation.total_lines > super::accumulator::BASH_PRUNE_LINE_THRESHOLD {
            let log_path = snapshot
                .full_output_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "temp log".to_string());
            let lines = snapshot.truncation.total_lines;
            let size = crate::tools::truncate::format_size(snapshot.truncation.total_bytes);
            format!(
                "{body}\n\n[Command completed successfully with exit code 0 ({lines} lines, {size}). Full log: {log_path}]"
            )
        } else {
            body
        };
        ToolResult::success(res)
    } else {
        let exit_code = status.code().unwrap_or(-1);
        let msg = format!("Command exited with code {exit_code}");
        let res = if raw_text.is_empty() {
            msg
        } else {
            let collapsed = super::normalize::collapse_consecutive_lines(raw_text);
            format!("{collapsed}\n\n{msg}")
        };
        ToolResult::error(res)
    }
}

async fn consume_stream_chunks<F: FnMut(&str)>(
    rx: &mut UnboundedReceiver<String>,
    on_chunk: &mut F,
    accumulator: &mut OutputAccumulator,
) {
    let mut chunk_count = 0usize;
    while let Some(chunk) = rx.recv().await {
        on_chunk(&chunk);
        accumulator.append(chunk.as_bytes());
        chunk_count += 1;
        if chunk_count.is_multiple_of(16) {
            tokio::task::yield_now().await;
        }
    }
}

async fn execute_process_loop<F: FnMut(&str)>(
    rx: &mut UnboundedReceiver<String>,
    on_chunk: &mut F,
    acc: &mut OutputAccumulator,
    cmd: &mut StreamingCommand,
) -> std::io::Result<std::process::ExitStatus> {
    consume_stream_chunks(rx, on_chunk, acc).await;
    cmd.drain_tasks().await;
    acc.finish();
    cmd.wait().await
}

struct TimeoutFallbackContext<'a, F: FnMut(&str)> {
    cmd: StreamingCommand,
    rx: UnboundedReceiver<String>,
    on_chunk: F,
    acc: OutputAccumulator,
    args: &'a BashArgs,
    timeout_sec: u64,
}

async fn handle_timeout_fallback<F: FnMut(&str)>(mut ctx: TimeoutFallbackContext<'_, F>) -> ToolResult {
    if ctx.args.background == Some(false) {
        ctx.cmd.cancel().await;
        while let Ok(chunk) = ctx.rx.try_recv() {
            (ctx.on_chunk)(&chunk);
            ctx.acc.append(chunk.as_bytes());
        }
        ctx.acc.finish();
        let snapshot = ctx.acc.snapshot();
        let output = snapshot.formatted_text.trim();
        let msg = format!("Command timed out after {} seconds", ctx.timeout_sec);
        let res = if output.is_empty() {
            msg
        } else {
            format!("{output}\n\n{msg}")
        };
        return ToolResult::error(res);
    }

    let job_id = format!("bash-{}", uuid::Uuid::new_v4().simple());
    crate::tools::wait::JobManager::global().register(job_id.clone(), ctx.args.command.clone());
    let command_str = ctx.args.command.clone();
    dispatch_background_task(ctx.cmd, ctx.rx, ctx.acc, job_id.clone());

    let notice = format_background_notice(&job_id);
    ToolResult::error(format!(
        "[Command running in background after {}s timeout]\nJob ID: {job_id}\nCommand: {command_str}\nCommand timed out after {} seconds\n\n{notice}",
        ctx.timeout_sec, ctx.timeout_sec
    ))
}

async fn run_with_timeout<F>(
    base_dir: &Path,
    args: &BashArgs,
    mut on_chunk: F,
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
    let exec = execute_process_loop(&mut rx, &mut on_chunk, &mut acc, &mut cmd);
    match tokio::time::timeout(Duration::from_secs(timeout_sec), exec).await {
        Ok(Ok(status)) => Ok(format_exit_result(status, &acc)),
        Ok(Err(e)) => Ok(ToolResult::error(format!(
            "Failed waiting for command '{}': {e}",
            args.command
        ))),
        Err(_) => {
            let ctx = TimeoutFallbackContext {
                cmd,
                rx,
                on_chunk,
                acc,
                args,
                timeout_sec,
            };
            Ok(handle_timeout_fallback(ctx).await)
        }
    }
}

pub async fn run_command_streaming<F>(base_dir: &Path, args: &BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
where
    F: FnMut(&str) + Send + 'static,
{
    if args.background.unwrap_or(false) {
        let (cmd, rx) = match setup_command_child(base_dir, &args.command) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let job_id = format!("bash-{}", uuid::Uuid::new_v4().simple());
        crate::tools::wait::JobManager::global().register(job_id.clone(), args.command.clone());
        let command_str = args.command.clone();
        let acc = OutputAccumulator::new();
        dispatch_background_task(cmd, rx, acc, job_id.clone());

        let notice = format_background_notice(&job_id);
        return Ok(ToolResult::success(format!(
            "[Command dispatched to background]\nJob ID: {job_id}\nCommand: {command_str}\n\n{notice}"
        )));
    }

    let timeout_sec = args.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_SEC);
    run_with_timeout(base_dir, args, on_chunk, timeout_sec).await
}
