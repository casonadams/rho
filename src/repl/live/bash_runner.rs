use std::process::Stdio;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use rho_engine::process::{ProcessTreeGuard, isolate_group};
use rho_engine::tools::bash::{OutputAccumulator, OutputSnapshot};
use rho_harness_core::presentation::ToolLine;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS};
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InputAction, TerminalBackend, TerminalController, map_key};

const STREAM_REDRAW_INTERVAL: Duration = Duration::from_millis(50);

pub struct UserBashResult {
    pub output: String,
    pub is_cancelled: bool,
    pub is_error: bool,
}

struct BashOutcome {
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub args_val: serde_json::Value,
}

struct RunningCommand {
    pub guard: ProcessTreeGuard,
    pub stdout_task: JoinHandle<()>,
    pub stderr_task: JoinHandle<()>,
}

impl RunningCommand {
    fn spawn(cmd: &str) -> std::io::Result<(Self, UnboundedReceiver<String>)> {
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

    async fn cancel(&mut self) {
        self.stdout_task.abort();
        self.stderr_task.abort();
        self.guard.kill().await;
    }

    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.guard.wait().await
    }

    async fn drain_tasks(&mut self) {
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

struct StreamProgress {
    spinner_tick: usize,
    last_redraw: Instant,
    needs_redraw: bool,
}

impl StreamProgress {
    fn new() -> Self {
        Self {
            spinner_tick: 0,
            last_redraw: Instant::now(),
            needs_redraw: false,
        }
    }

    fn on_chunk(&mut self) -> bool {
        self.needs_redraw = true;
        if self.last_redraw.elapsed() >= STREAM_REDRAW_INTERVAL {
            self.last_redraw = Instant::now();
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    fn on_tick<B: TerminalBackend>(&mut self, controller: &mut TerminalController<B>) -> bool {
        self.spinner_tick += 1;
        let spinner_advanced = if self.spinner_tick >= SPINNER_FRAME_INTERVALS {
            self.spinner_tick = 0;
            controller.advance_spinner();
            !matches!(controller.state().footer().activity, Activity::Idle)
        } else {
            false
        };
        if self.needs_redraw && self.last_redraw.elapsed() >= STREAM_REDRAW_INTERVAL {
            self.needs_redraw = false;
            self.last_redraw = Instant::now();
            true
        } else {
            spinner_advanced
        }
    }
}

fn finalize_run(
    chunk_rx: &mut UnboundedReceiver<String>,
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

fn completed_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome, code: i32) -> (ToolLine, UserBashResult) {
    let is_error = code != 0;
    let output = format_bash_output(snapshot, code);
    let summary = if is_error {
        format!("exit {code}")
    } else {
        "completed".to_string()
    };
    (
        ToolLine {
            name: "bash".to_string(),
            arguments: outcome.args_val,
            is_error,
            output: output.clone(),
            output_summary: summary,
            duration_ms: Some(outcome.duration_ms),
        },
        UserBashResult {
            output,
            is_cancelled: false,
            is_error,
        },
    )
}

fn cancelled_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome) -> (ToolLine, UserBashResult) {
    let output = format_cancel_output(snapshot);
    (
        ToolLine {
            name: "bash".to_string(),
            arguments: outcome.args_val,
            is_error: true,
            output: output.clone(),
            output_summary: "(cancelled)".to_string(),
            duration_ms: Some(outcome.duration_ms),
        },
        UserBashResult {
            output,
            is_cancelled: true,
            is_error: true,
        },
    )
}

fn finish_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome) -> (ToolLine, UserBashResult) {
    match outcome.exit_code {
        Some(code) => completed_bash_result(snapshot, outcome, code),
        None => cancelled_bash_result(snapshot, outcome),
    }
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

type UiEvents = UnboundedReceiver<crate::ui::interactive::UiEvent>;
type ChunkRx = UnboundedReceiver<String>;

struct StreamBuffers {
    chunk_rx: ChunkRx,
    accumulator: OutputAccumulator,
    progress: StreamProgress,
}

struct SpawnOutcome {
    args_val: serde_json::Value,
    duration_ms: u64,
}

struct BashRun<'a, B: TerminalBackend> {
    renderer: &'a TerminalRenderer,
    controller: &'a mut TerminalController<B>,
    batch: LiveBatch,
    events: &'a mut UiEvents,
}

impl<B: TerminalBackend> BashRun<'_, B> {
    fn drain_and_flush(&mut self, spinner: bool) -> Result<()> {
        self.batch.drain_events(self.controller, self.events)?;
        self.batch.flush(self.controller, spinner)
    }

    async fn handle_input_action(&mut self, action: InputAction, running: &mut RunningCommand) -> Result<bool> {
        if action == InputAction::Cancel {
            running.cancel().await;
            return Ok(true);
        }
        if let InputAction::ToggleExpandTools | InputAction::ThinkingToggle = action {
            self.apply_ui_toggle(action)?;
        }
        Ok(false)
    }

    fn apply_ui_toggle(&mut self, action: InputAction) -> Result<()> {
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
        let resized = self.controller.refresh_size()?;
        if resized {
            self.renderer.set_width(self.controller.width());
        }
        if stream.progress.on_tick(self.controller) || expired || resized {
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

struct BashStreamState<'a, 'b, B: TerminalBackend> {
    run: &'a mut BashRun<'b, B>,
    running: RunningCommand,
    stream: StreamBuffers,
    input: &'a mut super::TerminalInputReader,
    frame: tokio::time::Interval,
    started: Instant,
    args_val: serde_json::Value,
    exit: Option<Option<i32>>,
}

impl<B: TerminalBackend> BashStreamState<'_, '_, B> {
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
            Event::Resize(cols, rows) => {
                let resized = self.run.controller.resize_to(usize::from(cols), usize::from(rows))?
                    || self.run.controller.refresh_size()?;
                if resized {
                    self.run.renderer.set_width(self.run.controller.width());
                }
                self.run.drain_and_flush(true)?;
                Ok(false)
            }
            Event::FocusGained => {
                let resized = self.run.controller.refresh_size()?;
                if resized {
                    self.run.renderer.set_width(self.run.controller.width());
                }
                if !self.run.controller.focused() || resized {
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

async fn spawn_failure<B: TerminalBackend>(
    run: &mut BashRun<'_, B>,
    cmd: &str,
    e: &std::io::Error,
    args_val: serde_json::Value,
    started: Instant,
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

fn build_stream_state<'a, 'b, B: TerminalBackend>(
    run: &'a mut BashRun<'b, B>,
    running: RunningCommand,
    chunk_rx: ChunkRx,
    input: &'a mut super::TerminalInputReader,
    started: Instant,
    args_val: serde_json::Value,
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

pub async fn run_user_bash<B: TerminalBackend>(
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
        Err(e) => return spawn_failure(&mut run, cmd, &e, args_val, started).await,
    };

    run.drain_and_flush(true)?;

    let mut state = build_stream_state(&mut run, running, chunk_rx, io.input, started, args_val);
    let exit_code = state.stream_until_exit().await?;
    state.finish(exit_code).await
}
