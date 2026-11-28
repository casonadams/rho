//! Another agent shown in place of the main agent reads exactly like the main agent: the same
//! turn, driven through the turn's sink into the main agent's renderer or into an agent's record
//! shown in the view, leaves the same rows on the screen at every step.

use super::screen_sim::ScreenBackend;
use crate::repl::live::agent_focus::{refresh, show_agent};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, InteractiveUi, PendingUiBatch, UiEvent};
use rho_engine::engine::runner::{TerminalApprovalSink, TerminalSinkConfig, ToolFinishDetails};
use rho_engine::tools::subagent::AgentPresenter;
use rho_harness_core::agents::{AgentHandle, AgentMonitor, AgentStatus};
use rho_harness_core::presentation::Presenter;
use rho_harness_core::presentation::stream::ToolStreamSink;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

const TASK: &str = "Audit the parser";

fn screen() -> TerminalController<ScreenBackend> {
    TerminalController::pinned_to_bottom(ScreenBackend::new(70, 80), InteractiveState::default()).unwrap()
}

/// The rows above the live region, without the durations, which each side times on its own.
fn transcript_rows(controller: &TerminalController<ScreenBackend>) -> Vec<String> {
    let screen = controller.backend.text();
    let region = controller.rendered().expect("a drawn region").height();
    screen[..screen.len() - region]
        .iter()
        .filter(|row| !row.contains("Elapsed ") && !row.contains("Took "))
        .cloned()
        .collect()
}

fn sink(presenter: &Arc<dyn Presenter>) -> Arc<TerminalApprovalSink> {
    let config = TerminalSinkConfig {
        run_tracker: rho_engine::engine::metrics::RunTracker::default(),
    };
    TerminalApprovalSink::with_session(presenter, config, None)
}

/// The main agent: its renderer's events applied the way the live loop applies them.
struct Main {
    presenter: Arc<dyn Presenter>,
    sink: Arc<TerminalApprovalSink>,
    events: UnboundedReceiver<UiEvent>,
    controller: TerminalController<ScreenBackend>,
}

impl Main {
    fn new() -> Self {
        let (ui, events) = InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let controller = screen();
        renderer.set_width(controller.width());
        let presenter: Arc<dyn Presenter> = Arc::new(renderer);
        Self {
            sink: sink(&presenter),
            presenter,
            events,
            controller,
        }
    }

    fn rows(&mut self) -> Vec<String> {
        let mut pending = PendingUiBatch::new(64 * 1024);
        while let Ok(event) = self.events.try_recv() {
            pending.push(event);
        }
        self.controller.apply_output(pending.drain()).unwrap();
        transcript_rows(&self.controller)
    }
}

/// A subagent: its turn recorded on the monitor and shown in the main terminal's agent view.
struct Shown {
    presenter: Arc<dyn Presenter>,
    sink: Arc<TerminalApprovalSink>,
    monitor: AgentMonitor,
    handle: AgentHandle,
    controller: TerminalController<ScreenBackend>,
}

impl Shown {
    fn new() -> Self {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("subagents", "audit", "acme/smol", 10);
        let presenter: Arc<dyn Presenter> = Arc::new(AgentPresenter::new(Some(handle.clone())));
        let mut controller = screen();
        show_agent(&mut controller, Some(&monitor), Some(handle.id())).unwrap();
        Self {
            sink: sink(&presenter),
            presenter,
            monitor,
            handle,
            controller,
        }
    }

    fn rows(&mut self) -> Vec<String> {
        refresh(&mut self.controller, &self.monitor).unwrap();
        transcript_rows(&self.controller)
    }
}

struct Turn {
    main: Main,
    shown: Shown,
}

impl Turn {
    /// Takes the same `step` on both agents and checks their screens still read the same.
    fn step(&mut self, what: &str, step: impl Fn(&dyn Presenter, &TerminalApprovalSink)) -> Vec<String> {
        step(self.main.presenter.as_ref(), &self.main.sink);
        step(self.shown.presenter.as_ref(), &self.shown.sink);
        let main = self.main.rows();
        assert_eq!(self.shown.rows(), main, "{what}");
        main
    }

    fn run_bash(&mut self, command: &str, output: &str) {
        let args = serde_json::json!({ "command": command });
        self.step("the tool starts", |_, sink| {
            sink.tool_start("bash", &args);
            sink.tool_chunk(output.to_string());
        });
        self.step("the tool finishes", |_, sink| {
            sink.tool_finished(ToolFinishDetails {
                name: "bash",
                arguments: &args,
                output,
                is_error: false,
            });
        });
    }
}

#[test]
fn a_shown_agents_turn_leaves_the_same_rows_as_the_main_agents_at_every_step() {
    let mut turn = Turn {
        main: Main::new(),
        shown: Shown::new(),
    };
    turn.step("the task", |presenter, _| presenter.print_user_block(TASK));
    for token in ["Checking the **parser** ", "before I `answer`, ", "line by line."] {
        turn.step(&format!("thinking {token:?}"), |_, sink| sink.emit_reasoning(token));
    }
    for token in [
        "The **parser** ",
        "looks fine:\n\n",
        "- tokens\n",
        "- errors\n\n",
        "Running it.",
    ] {
        turn.step(&format!("reply {token:?}"), |_, sink| sink.emit_text(token));
    }
    turn.run_bash("cargo test", "ok\n");
    turn.step("thinking after the tool", |_, sink| {
        sink.emit_reasoning("Now the **summary**.")
    });
    turn.step("the closing reply", |_, sink| sink.emit_text("All **good**.\n"));

    turn.main.sink.flush_display();
    turn.shown.sink.flush_display();
    turn.shown.handle.finish(AgentStatus::Done);
    let rows = turn.main.rows();
    assert_eq!(turn.shown.rows(), rows, "the finished turn");
    let text = rows.join("\n");
    assert!(
        text.contains("The parser looks fine:") && text.contains("All good."),
        "{text}"
    );
    assert!(!text.contains("**"), "markdown is rendered: {text}");
}
