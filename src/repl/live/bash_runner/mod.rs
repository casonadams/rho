mod command;
mod format;
mod progress;

pub use format::UserBashResult;

use crossterm::event::Event;
use rho_engine::tools::bash::{OutputAccumulator, OutputSnapshot};
use rho_harness_core::presentation::ToolLine;
use std::time::Instant;

use command::RunningCommand;
use format::{BashOutcome, finish_bash_result};
use progress::StreamProgress;

use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InputAction, map_key};

fn finalize_run(
    chunk_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    accumulator: &mut OutputAccumulator,
    renderer: &TerminalRenderer,
) -> OutputSnapshot {
    while let Ok(chunk) = chunk_rx.try_recv() {
        accumulator.append(chunk.as_bytes());
        renderer.tool_chunk(&chunk);
    }
    accumulator.finish();
    accumulator.snapshot()
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

    let (mut running, mut chunk_rx) = match RunningCommand::spawn(cmd) {
        Ok(res) => res,
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

    batch.drain_events(controller, io.events)?;
    batch.flush(controller, true)?;

    let mut accumulator = OutputAccumulator::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut progress = StreamProgress::new();
    let input_reader = &mut *io.input;

    loop {
        tokio::select! {
            biased;
            event = input_reader.recv() => {
                if let Some(Ok(Event::Key(key))) = event {
                    match map_key(key) {
                        InputAction::Cancel => {
                            running.cancel().await;
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
                while let Ok(more) = chunk_rx.try_recv() {
                    accumulator.append(more.as_bytes());
                    renderer.tool_chunk(&more);
                }
                if progress.on_chunk() {
                    batch.drain_events(controller, io.events)?;
                    batch.flush(controller, true)?;
                }
            }
            _ = frame.tick() => {
                if progress.on_tick(controller) {
                    batch.drain_events(controller, io.events)?;
                    batch.flush(controller, true)?;
                }
            }
            res = running.wait() => {
                running.drain_tasks().await;
                let snapshot = finalize_run(&mut chunk_rx, &mut accumulator, renderer);
                let exit_code = res.ok().and_then(|s| s.code()).unwrap_or(-1);
                let (line, result) = finish_bash_result(
                    &snapshot,
                    BashOutcome {
                        exit_code: Some(exit_code),
                        duration_ms: started.elapsed().as_millis() as u64,
                        args_val,
                    },
                );
                renderer.finish_tool_line(line);
                batch.drain_events(controller, io.events)?;
                batch.flush(controller, false)?;
                return Ok(result);
            }
        }
    }

    let snapshot = finalize_run(&mut chunk_rx, &mut accumulator, renderer);
    let (line, result) = finish_bash_result(
        &snapshot,
        BashOutcome {
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            args_val,
        },
    );
    renderer.finish_tool_line(line);
    batch.drain_events(controller, io.events)?;
    batch.flush(controller, false)?;

    Ok(result)
}
