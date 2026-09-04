use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InputAction, map_key};
use crossterm::event::Event;
use rho_engine::process::{ProcessTreeGuard, isolate_group};
use rho_engine::tools::bash::{OutputAccumulator, OutputSnapshot};
use rho_harness_core::presentation::ToolLine;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

pub struct UserBashResult {
    pub output: String,
    pub is_cancelled: bool,
    pub is_error: bool,
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

fn format_bash_output(snapshot: &OutputSnapshot, exit_code: i32) -> String {
    let output_trimmed = snapshot.formatted_text.trim();
    if exit_code != 0 {
        let status_msg = format!("Command exited with code {exit_code}");
        if output_trimmed.is_empty() {
            status_msg
        } else {
            format!("{output_trimmed}\n\n{status_msg}")
        }
    } else if output_trimmed.is_empty() {
        "[Command completed with exit code 0 (no output)]".to_string()
    } else {
        snapshot.formatted_text.clone()
    }
}

fn format_cancel_output(snapshot: &OutputSnapshot) -> String {
    let output_trimmed = snapshot.formatted_text.trim();
    if output_trimmed.is_empty() {
        "(cancelled)".to_string()
    } else {
        format!("{output_trimmed}\n(cancelled)")
    }
}

pub async fn run_user_bash<B: crate::ui::interactive::TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<UserBashResult> {
    let started = Instant::now();
    let args_val = serde_json::json!({ "command": cmd });
    renderer.start_tool_run("bash", &args_val);

    let mut batch = LiveBatch::new();
    let controller = &mut *io.controller;

    let mut command = configure_shell_command(cmd);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let error_msg = format!("Failed to spawn command '{cmd}': {e}");
            renderer.finish_tool_line(ToolLine {
                name: "bash".to_string(),
                arguments: args_val,
                is_error: true,
                output: error_msg.clone(),
                output_summary: "spawn error".to_string(),
                duration_ms: Some(started.elapsed().as_millis() as u64),
            });
            batch.drain_events(controller, io.events)?;
            batch.flush(controller, false)?;
            return Ok(UserBashResult {
                output: error_msg,
                is_cancelled: false,
                is_error: true,
            });
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut guard = ProcessTreeGuard::new(child);

    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout_task = spawn_stream_reader(stdout, chunk_tx.clone());
    let stderr_task = spawn_stream_reader(stderr, chunk_tx);

    let mut accumulator = OutputAccumulator::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut spinner_tick = 0_usize;

    let input_reader = &mut *io.input;

    loop {
        tokio::select! {
            biased;
            event = input_reader.recv() => {
                if let Some(Ok(Event::Key(key))) = event {
                    match map_key(key) {
                        InputAction::Cancel => {
                            stdout_task.abort();
                            stderr_task.abort();
                            guard.kill().await;
                            break;
                        }
                        InputAction::ToggleExpandTools => {
                            let _ = controller.toggle_tools_expanded();
                            batch.flush(controller, true)?;
                        }
                        _ => {}
                    }
                }
            }
            Some(chunk) = chunk_rx.recv() => {
                accumulator.append(chunk.as_bytes());
                renderer.tool_chunk(&chunk);
                batch.flush(controller, true)?;
            }
            _ = frame.tick() => {
                spinner_tick += 1;
                let spinner_advanced = if spinner_tick >= SPINNER_FRAME_INTERVALS {
                    spinner_tick = 0;
                    controller.advance_spinner();
                    !matches!(controller.state().footer().activity, Activity::Idle)
                } else {
                    false
                };
                batch.flush(controller, spinner_advanced)?;
            }
            res = guard.wait() => {
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                while let Ok(chunk) = chunk_rx.try_recv() {
                    accumulator.append(chunk.as_bytes());
                    renderer.tool_chunk(&chunk);
                }
                batch.flush(controller, false)?;

                accumulator.finish();
                let snapshot = accumulator.snapshot();
                let duration_ms = started.elapsed().as_millis() as u64;
                let exit_code = res.ok().and_then(|s| s.code()).unwrap_or(-1);
                let is_error = exit_code != 0;
                let final_output = format_bash_output(&snapshot, exit_code);

                renderer.finish_tool_line(ToolLine {
                    name: "bash".to_string(),
                    arguments: args_val,
                    is_error,
                    output: final_output.clone(),
                    output_summary: if is_error { format!("exit {exit_code}") } else { "completed".to_string() },
                    duration_ms: Some(duration_ms),
                });
                batch.drain_events(controller, io.events)?;
                batch.flush(controller, false)?;

                return Ok(UserBashResult {
                    output: final_output,
                    is_cancelled: false,
                    is_error,
                });
            }
        }
    }

    while let Ok(chunk) = chunk_rx.try_recv() {
        accumulator.append(chunk.as_bytes());
    }
    accumulator.finish();
    let snapshot = accumulator.snapshot();
    let duration_ms = started.elapsed().as_millis() as u64;
    let cancel_output = format_cancel_output(&snapshot);

    renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: args_val,
        is_error: true,
        output: cancel_output.clone(),
        output_summary: "(cancelled)".to_string(),
        duration_ms: Some(duration_ms),
    });
    batch.drain_events(controller, io.events)?;
    batch.flush(controller, false)?;

    Ok(UserBashResult {
        output: cancel_output,
        is_cancelled: true,
        is_error: true,
    })
}
