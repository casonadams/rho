mod command;
mod format;
mod progress;

pub use format::UserBashResult;

use crossterm::event::Event;
use rho_engine::tools::bash::{OutputAccumulator, OutputSnapshot};
use rho_harness_core::presentation::ToolLine;
use std::time::Instant;

use command::RunningCommand;
use format::{BashOutcome, finalize_run, finish_bash_result};
use progress::StreamProgress;

use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InputAction, map_key};

type UiEvents = tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>;
type ChunkRx = tokio::sync::mpsc::UnboundedReceiver<String>;

struct StreamBuffers {
    chunk_rx: ChunkRx,
    accumulator: OutputAccumulator,
    progress: StreamProgress,
}

struct SpawnOutcome {
    args_val: serde_json::Value,
    duration_ms: u64,
}

struct BashRun<'a, B: crate::ui::interactive::TerminalBackend> {
    renderer: &'a TerminalRenderer,
    controller: &'a mut crate::ui::interactive::TerminalController<B>,
    batch: LiveBatch,
    events: &'a mut UiEvents,
}

impl<B: crate::ui::interactive::TerminalBackend> BashRun<'_, B> {
    fn drain_and_flush(&mut self, spinner: bool) -> Result<()> {
        self.batch.drain_events(self.controller, self.events)?;
        self.batch.flush(self.controller, spinner)
    }

    async fn handle_input_action(&mut self, action: InputAction, running: &mut RunningCommand) -> Result<bool> {
        if action == InputAction::Cancel {
            running.cancel().await;
            return Ok(true);
        }
        if let InputAction::ToggleExpandTools | InputAction::ThinkingToggle | InputAction::BlockStyleToggle = action {
            self.apply_ui_toggle(action)?;
        }
        Ok(false)
    }

    fn apply_ui_toggle(&mut self, action: InputAction) -> Result<()> {
        if action == InputAction::BlockStyleToggle {
            if let Ok(new_style) = self.controller.toggle_block_style() {
                let label = match new_style {
                    crate::ui::theme::BlockStyle::Border => "border",
                    crate::ui::theme::BlockStyle::Solid => "solid",
                };
                self.controller.set_system_message(format!("Block style: {label}"));
            }
            return Ok(());
        }
        let (label, expanded) = if action == InputAction::ToggleExpandTools {
            let expanded = !self.controller.tools_expanded();
            ("Tool output", expanded)
        } else {
            let hidden = !self.controller.hide_thinking();
            ("Thinking blocks", !hidden)
        };
        let state = if expanded { "expanded" } else { "collapsed" };
        self.controller.set_system_message(format!("{label}: {state}"));
        if action == InputAction::ToggleExpandTools {
            self.controller.set_tools_expanded(expanded)?;
        } else {
            self.controller.set_hide_thinking(!expanded)?;
        }
        Ok(())
    }

    fn handle_chunk(&mut self, chunk: String, stream: &mut StreamBuffers) -> Result<()> {
        stream.accumulator.append(chunk.as_bytes());
        self.renderer.tool_chunk(&chunk);
        while let Ok(more) = stream.chunk_rx.try_recv() {
            stream.accumulator.append(more.as_bytes());
            self.renderer.tool_chunk(&more);
        }
        if stream.progress.on_chunk() {
            self.drain_and_flush(true)?;
        }
        Ok(())
    }

    fn handle_frame_tick(&mut self, stream: &mut StreamBuffers) -> Result<()> {
        let expired = self.controller.check_system_message_expiration();
        if stream.progress.on_tick(self.controller) || expired {
            self.drain_and_flush(true)?;
        }
        Ok(())
    }

    fn emit_result(&mut self, snapshot: &OutputSnapshot, outcome: BashOutcome) -> Result<UserBashResult> {
        let (line, result) = finish_bash_result(snapshot, outcome);
        self.renderer.finish_tool_line(line);
        self.drain_and_flush(false)?;
        Ok(result)
    }
}

enum StreamStep {
    Key(Option<std::io::Result<Event>>),
    Chunk(String),
    Frame,
    Exit(Option<i32>),
}

async fn select_input_chunk(input: &mut super::TerminalInputReader, chunk_rx: &mut ChunkRx) -> StreamStep {
    tokio::select! {
        biased;
        event = input.recv() => StreamStep::Key(event),
        Some(chunk) = chunk_rx.recv() => StreamStep::Chunk(chunk),
    }
}

async fn select_frame_exit(frame: &mut tokio::time::Interval, running: &mut RunningCommand) -> StreamStep {
    tokio::select! {
        biased;
        _ = frame.tick() => StreamStep::Frame,
        res = running.wait() => StreamStep::Exit(exit_code_of(res)),
    }
}

fn exit_code_of(res: std::io::Result<std::process::ExitStatus>) -> Option<i32> {
    Some(res.ok().and_then(|s| s.code()).unwrap_or(-1))
}

struct BashStreamState<'a, 'b, B: crate::ui::interactive::TerminalBackend> {
    run: &'a mut BashRun<'b, B>,
    running: RunningCommand,
    stream: StreamBuffers,
    input: &'a mut super::TerminalInputReader,
    frame: tokio::time::Interval,
    started: Instant,
    args_val: serde_json::Value,
    exit: Option<Option<i32>>,
}

impl<B: crate::ui::interactive::TerminalBackend> BashStreamState<'_, '_, B> {
    fn outcome(&self, exit_code: Option<i32>) -> BashOutcome {
        BashOutcome {
            exit_code,
            duration_ms: self.started.elapsed().as_millis() as u64,
            args_val: self.args_val.clone(),
        }
    }

    async fn finish(mut self, exit_code: Option<i32>) -> Result<UserBashResult> {
        self.running.drain_tasks().await;
        let snapshot = finalize_run(
            &mut self.stream.chunk_rx,
            &mut self.stream.accumulator,
            self.run.renderer,
        );
        self.run.emit_result(&snapshot, self.outcome(exit_code))
    }

    async fn handle_step(&mut self, step: StreamStep) -> Result<bool> {
        match step {
            StreamStep::Key(event) => self.handle_key_event(event).await,
            StreamStep::Chunk(chunk) => {
                self.run.handle_chunk(chunk, &mut self.stream)?;
                Ok(false)
            }
            StreamStep::Frame => {
                self.run.handle_frame_tick(&mut self.stream)?;
                Ok(false)
            }
            StreamStep::Exit(code) => {
                self.exit = Some(code);
                Ok(true)
            }
        }
    }

    async fn handle_key_event(&mut self, event: Option<std::io::Result<Event>>) -> Result<bool> {
        let Some(Ok(event)) = event else {
            return Ok(false);
        };
        match event {
            Event::FocusGained => {
                if !self.run.controller.focused() {
                    self.run.controller.set_focused(true);
                    self.run.drain_and_flush(true)?;
                }
                Ok(false)
            }
            Event::FocusLost => {
                if self.run.controller.focused() {
                    self.run.controller.set_focused(false);
                    self.run.drain_and_flush(true)?;
                }
                Ok(false)
            }
            Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release => {
                self.run.handle_input_action(map_key(key), &mut self.running).await
            }
            _ => Ok(false),
        }
    }

    async fn select_next(&mut self) -> StreamStep {
        tokio::select! {
            biased;
            step = select_input_chunk(self.input, &mut self.stream.chunk_rx) => step,
            step = select_frame_exit(&mut self.frame, &mut self.running) => step,
        }
    }

    async fn stream_until_exit(&mut self) -> Result<Option<i32>> {
        loop {
            let step = self.select_next().await;
            if self.handle_step(step).await? {
                return Ok(self.exit.take().flatten());
            }
        }
    }
}

async fn spawn_failure<B: crate::ui::interactive::TerminalBackend>(
    run: &mut BashRun<'_, B>,
    (cmd, e, args_val, started): (&str, &std::io::Error, serde_json::Value, Instant),
) -> Result<UserBashResult> {
    let outcome = SpawnOutcome {
        args_val,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    let error_msg = format!("Failed to spawn command '{cmd}': {e}");
    run.renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: outcome.args_val,
        is_error: true,
        output: error_msg.clone(),
        output_summary: "spawn error".to_string(),
        duration_ms: Some(outcome.duration_ms),
    });
    run.drain_and_flush(false)?;
    Ok(UserBashResult {
        output: error_msg,
        is_cancelled: false,
        is_error: true,
    })
}

fn build_stream_state<'a, 'b, B: crate::ui::interactive::TerminalBackend>(
    run: &'a mut BashRun<'b, B>,
    (running, chunk_rx): (RunningCommand, ChunkRx),
    (input, started, args_val): (&'a mut super::TerminalInputReader, Instant, serde_json::Value),
) -> BashStreamState<'a, 'b, B> {
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    BashStreamState {
        run,
        running,
        stream: StreamBuffers {
            chunk_rx,
            accumulator: OutputAccumulator::new(),
            progress: StreamProgress::new(),
        },
        input,
        frame,
        started,
        args_val,
        exit: None,
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

    let mut run = BashRun {
        renderer,
        controller: io.controller,
        batch: LiveBatch::new(),
        events: io.events,
    };

    let (running, chunk_rx) = match RunningCommand::spawn(cmd) {
        Ok(res) => res,
        Err(e) => return spawn_failure(&mut run, (cmd, &e, args_val, started)).await,
    };

    run.drain_and_flush(true)?;

    let mut state = build_stream_state(&mut run, (running, chunk_rx), (io.input, started, args_val));
    let exit_code = state.stream_until_exit().await?;
    state.finish(exit_code).await
}
