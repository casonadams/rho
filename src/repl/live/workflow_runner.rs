use std::collections::BTreeMap;
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
use crate::repl::live::LiveIo;
use crate::repl::live::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
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
}

impl<B: TerminalBackend> WorkflowView<'_, '_, B> {
    fn flush(&mut self, spinner: bool) -> Result<()> {
        self.batch.drain_events(self.io.controller, self.io.events)?;
        self.batch.flush(self.io.controller, spinner)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if agents::handle_agents_key(self.io.controller, key, &self.monitor)?
            || agents::try_open_agents(self.io.controller, &key, &self.monitor)?
        {
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

    fn handle_frame(&mut self) -> Result<()> {
        let resized = self.io.controller.refresh_size()?;
        sync_renderer_width(self.renderer, self.io.controller, resized);
        self.io.controller.check_system_message_expiration();
        agents::refresh_agent_modals(self.io.controller, &self.monitor);
        self.flush(true)
    }
}

fn summary(run: &WorkflowRun) -> String {
    run.report.lines().next().unwrap_or_default().to_string()
}

pub(crate) async fn run_user_workflow<B: TerminalBackend>(
    renderer: &TerminalRenderer,
    engine: &AgentEngine,
    name: &str,
    inputs: BTreeMap<String, String>,
    io: &mut LiveIo<'_, B>,
) -> Result<()> {
    let started = Instant::now();
    let arguments = serde_json::json!({ "name": name, "inputs": inputs });
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
    let workflow = name.to_string();
    let mut task = tokio::spawn(async move { runner.run(&workflow, inputs).await });
    let mut view = WorkflowView {
        renderer,
        io,
        batch: LiveBatch::new(),
        monitor: engine.agent_monitor().clone(),
        cancel,
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
    });
    renderer.finish_tool_line(ToolLine {
        name: "workflow".to_string(),
        arguments,
        is_error: !run.ok,
        output_summary: summary(&run),
        output: run.report,
        duration_ms: Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        file_change: None,
    });
    view.flush(false)
}
