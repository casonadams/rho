use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{Event, KeyEvent, KeyEventKind};
use rho_engine::engine::AgentEngine;
use rho_engine::engine::runner::CancellationSignal;
use rho_engine::tools::WorkflowRun;
use rho_harness_core::agents::AgentMonitor;
use rho_harness_core::presentation::{ToolLine, ToolStreamPort, ToolStreamSink};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::error::Result;
use crate::repl::commands::types::WorkflowLaunch;
use crate::repl::live::LiveIo;
use crate::repl::live::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVAL};
use crate::repl::live::idle::dispatch::sync_renderer_width;
use crate::repl::live::modal::agents;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InputAction, TerminalBackend, map_key};

struct ChannelSink(UnboundedSender<String>);

impl ToolStreamSink for ChannelSink {
    fn tool_chunk(&self, chunk: String) {
        let _ = self.0.send(chunk);
    }
}

pub(crate) fn progress_channel() -> (ToolStreamPort, UnboundedReceiver<String>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (ToolStreamPort::new(Some(Arc::new(ChannelSink(tx)))), rx)
}

struct WorkflowView<'a, 'b, B: TerminalBackend> {
    renderer: &'a TerminalRenderer,
    io: &'a mut LiveIo<'b, B>,
    batch: LiveBatch,
    monitor: AgentMonitor,
    cancel: CancellationSignal,
    last_spinner: Instant,
}

impl<B: TerminalBackend> WorkflowView<'_, '_, B> {
    fn flush(&mut self, spinner: bool) -> Result<()> {
        self.batch.drain_events(self.io.controller, self.io.events)?;
        self.batch.flush(self.io.controller, spinner)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        let agent_key = agents::handle_agents_key(self.io.controller, key, &self.monitor)?;
        match &agent_key {
            agents::AgentKey::Stop(_) => {
                self.io
                    .controller
                    .set_system_message("Workflow agents stop with the workflow: press Esc to cancel it");
                self.flush(true)?;
            }
            agents::AgentKey::Escape(_) => {
                self.io.controller.leave_agent_view()?;
                self.flush(true)?;
            }
            agents::AgentKey::Ignored | agents::AgentKey::Handled => {}
        }
        if agent_key.consumed() || agents::try_open_agents(self.io.controller, &key, &self.monitor)? {
            return Ok(());
        }
        if map_key(key) == InputAction::Cancel {
            self.cancel.cancel();
            self.io.controller.set_system_message("Cancelling workflow…");
            self.flush(true)?;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Option<std::io::Result<Event>>) -> Result<()> {
        let Some(event) = event else {
            return Err(anyhow::anyhow!("Terminal input reader stopped").into());
        };
        match event? {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            Event::Resize(..) | Event::FocusGained => self.handle_frame(),
            _ => Ok(()),
        }
    }

    fn handle_chunk(&mut self, chunk: &str, chunks: &mut UnboundedReceiver<String>) -> Result<()> {
        self.renderer.tool_chunk(chunk);
        while let Ok(more) = chunks.try_recv() {
            self.renderer.tool_chunk(&more);
        }
        self.flush(true)
    }

    fn tick_spinner(&mut self) {
        if self.last_spinner.elapsed() >= SPINNER_FRAME_INTERVAL {
            self.last_spinner = Instant::now();
            self.io.controller.advance_spinner();
        }
    }

    fn handle_frame(&mut self) -> Result<()> {
        self.tick_spinner();
        let resized = self.io.controller.refresh_size()?;
        sync_renderer_width(self.renderer, self.io.controller, resized);
        self.io.controller.check_system_message_expiration();
        agents::refresh_agent_modals(self.io.controller, &self.monitor)?;
        self.flush(true)
    }
}

fn summary(run: &WorkflowRun) -> String {
    run.summary
        .as_deref()
        .unwrap_or(&run.report)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

pub(crate) async fn run_user_workflow<B: TerminalBackend>(
    renderer: &TerminalRenderer,
    engine: &AgentEngine,
    launch: WorkflowLaunch,
    io: &mut LiveIo<'_, B>,
) -> Result<()> {
    let started = Instant::now();
    let name = launch.label().to_string();
    let arguments = match &launch {
        WorkflowLaunch::Start { name, inputs } => serde_json::json!({ "name": name, "inputs": inputs }),
        WorkflowLaunch::Resume { run_id } => serde_json::json!({ "resume": run_id }),
    };
    let cancel = CancellationSignal::default();
    let (progress, mut chunks) = progress_channel();
    let runner = match engine.workflow_runner(progress, cancel.clone()).await {
        Ok(runner) => runner,
        Err(e) => {
            renderer.print_notice(&format!("  /workflow: {e}\n"));
            return Ok(());
        }
    };
    renderer.start_tool_run("workflow", &arguments);
    let mut task = tokio::spawn(async move { launch.run(&runner).await });
    let mut view = WorkflowView {
        renderer,
        io,
        batch: LiveBatch::new(),
        monitor: engine.agent_monitor().clone(),
        cancel,
        last_spinner: Instant::now(),
    };
    view.io
        .controller
        .set_system_message("Workflow running · ← watch agents · Esc cancel");
    view.flush(true)?;
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let joined = loop {
        tokio::select! {
            biased;
            event = view.io.input.recv() => view.handle_event(event)?,
            Some(chunk) = chunks.recv() => view.handle_chunk(&chunk, &mut chunks)?,
            _ = frame.tick() => view.handle_frame()?,
            joined = &mut task => break joined,
        }
    };
    while let Ok(chunk) = chunks.try_recv() {
        renderer.tool_chunk(&chunk);
    }
    let run = joined.unwrap_or_else(|e| WorkflowRun {
        ok: false,
        report: format!("Workflow `{name}` crashed: {e}"),
        summary: None,
        started: true,
    });
    if run.started {
        engine.hand_off_workflow_report(&name, &run.report);
    }
    let summary_text = run.summary.clone();
    renderer.finish_tool_line(ToolLine {
        name: "workflow".to_string(),
        arguments,
        is_error: !run.ok,
        output_summary: summary(&run),
        output: run.report,
        duration_ms: Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        file_change: None,
    });
    if let Some(text) = summary_text {
        renderer.print_notice(&format!("\n{text}\n"));
    }
    view.flush(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{Activity, InteractiveState, NullBackend, TerminalController};

    #[tokio::test]
    async fn frame_ticks_animate_the_working_spinner() {
        let mut state = InteractiveState::default();
        state.footer_mut().activity = Activity::Working;
        let mut controller = TerminalController::new(NullBackend, state).unwrap();
        let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
        let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
        let renderer = TerminalRenderer::with_ui(ui);
        let mut io = LiveIo {
            controller: &mut controller,
            events: &mut events,
            input: &mut input,
        };
        let mut view = WorkflowView {
            renderer: &renderer,
            io: &mut io,
            batch: LiveBatch::new(),
            monitor: AgentMonitor::default(),
            cancel: CancellationSignal::default(),
            last_spinner: Instant::now(),
        };
        view.flush(true).unwrap();
        let working_line =
            |view: &WorkflowView<'_, '_, NullBackend>| view.io.controller.rendered().unwrap().working_line.clone();
        let first = working_line(&view);

        view.last_spinner = Instant::now() - SPINNER_FRAME_INTERVAL;
        view.handle_frame().unwrap();

        assert!(first.contains("working"));
        assert_ne!(working_line(&view), first);
    }
}
