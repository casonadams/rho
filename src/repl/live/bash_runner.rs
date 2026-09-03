use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InputAction, map_key};
use crossterm::event::Event;
use rho_engine::process::{isolate_group, kill_tree};
use rho_harness_core::presentation::ToolLine;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub struct UserBashResult {
    pub output: String,
    pub is_cancelled: bool,
    pub is_error: bool,
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

    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };

    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(cmd);
        c
    };

    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);
    isolate_group(&mut command);

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

    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout_tx = chunk_tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut reader = stdout;
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 || stdout_tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).is_err() {
                break;
            }
        }
    });

    let stderr_tx = chunk_tx;
    let stderr_task = tokio::spawn(async move {
        let mut reader = stderr;
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 || stderr_tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).is_err() {
                break;
            }
        }
    });

    let mut output = String::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut spinner_tick = 0_usize;

    let input_reader = &mut *io.input;

    loop {
        tokio::select! {
            biased;
            Some(chunk) = chunk_rx.recv() => {
                output.push_str(&chunk);
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
            event = input_reader.recv() => {
                if let Some(Ok(Event::Key(key))) = event {
                    match map_key(key) {
                        InputAction::Cancel => {
                            kill_tree(&mut child).await;
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
            res = child.wait() => {
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                while let Ok(chunk) = chunk_rx.try_recv() {
                    output.push_str(&chunk);
                    renderer.tool_chunk(&chunk);
                }
                batch.flush(controller, false)?;

                let duration_ms = started.elapsed().as_millis() as u64;
                let exit_code = res.ok().and_then(|s| s.code()).unwrap_or(-1);
                let is_error = exit_code != 0;

                renderer.finish_tool_line(ToolLine {
                    name: "bash".to_string(),
                    arguments: args_val,
                    is_error,
                    output: output.clone(),
                    output_summary: if is_error { format!("exit {exit_code}") } else { "completed".to_string() },
                    duration_ms: Some(duration_ms),
                });
                batch.drain_events(controller, io.events)?;
                batch.flush(controller, false)?;

                return Ok(UserBashResult {
                    output,
                    is_cancelled: false,
                    is_error,
                });
            }
        }
    }

    let _ = stdout_task.await;
    let _ = stderr_task.await;
    while let Ok(chunk) = chunk_rx.try_recv() {
        output.push_str(&chunk);
        renderer.tool_chunk(&chunk);
    }
    batch.flush(controller, false)?;
    let duration_ms = started.elapsed().as_millis() as u64;

    renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: args_val,
        is_error: true,
        output: format!("{output}\n(cancelled)"),
        output_summary: "(cancelled)".to_string(),
        duration_ms: Some(duration_ms),
    });
    batch.drain_events(controller, io.events)?;
    batch.flush(controller, false)?;

    Ok(UserBashResult {
        output,
        is_cancelled: true,
        is_error: true,
    })
}
