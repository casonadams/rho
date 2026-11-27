//! Another agent shown in place of the main agent reads exactly like the main agent: the same
//! turn, streamed to the main agent's renderer or synced into the view frame by frame, leaves the
//! same rows on the screen at every step.

use super::screen_sim::ScreenBackend;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{
    InteractiveState, InteractiveUi, PendingUiBatch, StreamingText, ToolStartRequest, TranscriptItem, UiEvent,
    ViewFrame,
};
use crate::ui::render::stream::StreamKind;
use rho_harness_core::presentation::ToolLine;
use tokio::sync::mpsc::UnboundedReceiver;

const TASK: &str = "Audit the parser";
const THINKING: [&str; 3] = ["Checking the **parser** ", "before I `answer`, ", "line by line."];
const REPLY: [&str; 5] = [
    "The **parser** ",
    "looks fine:\n\n",
    "- tokens\n",
    "- errors\n\n",
    "Nothing to fix.",
];

fn screen() -> TerminalController<ScreenBackend> {
    TerminalController::pinned_to_bottom(ScreenBackend::new(70, 60), InteractiveState::default()).unwrap()
}

/// The rows above the live region, except the running card's elapsed time.
fn transcript_rows(controller: &TerminalController<ScreenBackend>) -> Vec<String> {
    let screen = controller.backend.text();
    let region = controller.rendered().expect("a drawn region").height();
    screen[..screen.len() - region]
        .iter()
        .filter(|row| !row.contains("Elapsed "))
        .cloned()
        .collect()
}

fn bash_args() -> serde_json::Value {
    serde_json::json!({ "command": "cargo test" })
}

fn bash_line() -> ToolLine {
    ToolLine::new("bash", bash_args())
        .with_output("ok\n")
        .with_output_summary("ok")
        .with_duration_ms(1200)
}

/// The main agent: its renderer's events applied the way the live loop applies them.
struct Main {
    renderer: TerminalRenderer,
    events: UnboundedReceiver<UiEvent>,
    controller: TerminalController<ScreenBackend>,
}

impl Main {
    fn new() -> Self {
        let (ui, events) = InteractiveUi::channel();
        let renderer = TerminalRenderer::with_ui(ui);
        let mut controller = screen();
        renderer.set_width(controller.width());
        controller
            .push_transcript_item(TranscriptItem::UserMessage(TASK.into()))
            .unwrap();
        Self {
            renderer,
            events,
            controller,
        }
    }

    fn apply(&mut self) -> Vec<String> {
        let mut pending = PendingUiBatch::new(64 * 1024);
        while let Ok(event) = self.events.try_recv() {
            pending.push(event);
        }
        self.controller.apply_output(pending.drain()).unwrap();
        transcript_rows(&self.controller)
    }
}

/// The shown agent: the frames its record turns into as the same turn goes on.
struct Shown {
    controller: TerminalController<ScreenBackend>,
    items: Vec<TranscriptItem>,
}

impl Shown {
    fn new() -> Self {
        let mut controller = screen();
        let items = vec![TranscriptItem::UserMessage(TASK.into())];
        let frame = ViewFrame {
            items: items.clone(),
            ..ViewFrame::default()
        };
        controller.enter_agent_view(1, "audit".into(), frame).unwrap();
        Self { controller, items }
    }

    fn sync(&mut self, frame: ViewFrame) -> Vec<String> {
        self.controller.sync_agent_view(frame).unwrap();
        transcript_rows(&self.controller)
    }

    fn streaming(&mut self, kind: StreamKind, text: &str) -> Vec<String> {
        let frame = ViewFrame {
            items: self.items.clone(),
            streaming: Some(StreamingText {
                kind,
                text: text.into(),
            }),
            ..ViewFrame::default()
        };
        self.sync(frame)
    }

    fn settle(&mut self, item: TranscriptItem) {
        self.items.push(item);
    }
}

#[test]
fn a_shown_agents_turn_leaves_the_same_rows_as_the_main_agents_at_every_step() {
    let mut main = Main::new();
    let mut shown = Shown::new();
    assert_eq!(
        shown.sync(ViewFrame {
            items: shown.items.clone(),
            ..ViewFrame::default()
        }),
        main.apply()
    );

    main.renderer.write_output("\n");
    let mut thinking = String::new();
    for token in THINKING {
        thinking.push_str(token);
        main.renderer.print_thinking_token(token);
        assert_eq!(
            shown.streaming(StreamKind::Thinking, &thinking),
            main.apply(),
            "thinking {thinking:?}"
        );
    }

    main.renderer.finish_thinking(&thinking);
    main.renderer.write_output("\n");
    main.renderer.write_output("\n");
    shown.settle(TranscriptItem::Thinking(thinking));
    let mut reply = String::new();
    for token in REPLY {
        reply.push_str(token);
        main.renderer.print_token(token);
        assert_eq!(
            shown.streaming(StreamKind::Reply, &reply),
            main.apply(),
            "reply {reply:?}"
        );
    }

    main.renderer.flush();
    main.renderer.start_tool_run("bash", &bash_args());
    main.renderer.tool_chunk("ok\n");
    shown.settle(TranscriptItem::AssistantText(reply));
    let running = ViewFrame {
        items: shown.items.clone(),
        running_tool: Some(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            arguments: bash_args(),
            file_change: None,
        }),
        running_output: "ok\n".into(),
        ..ViewFrame::default()
    };
    assert_eq!(shown.sync(running), main.apply(), "the running tool");

    main.renderer.finish_tool_line(bash_line());
    shown.settle(TranscriptItem::Tool(bash_line()));
    let done = ViewFrame {
        items: shown.items.clone(),
        ..ViewFrame::default()
    };
    let rows = shown.sync(done);
    assert_eq!(rows, main.apply(), "the finished turn");
    assert!(
        rows.iter().any(|row| row.contains("The parser looks fine:")),
        "{rows:#?}"
    );
}
