use std::collections::VecDeque;

use super::*;
use crate::ui::interactive::{Activity, InteractiveState, UiAction};

#[test]
fn pending_batch_preserves_text_and_keeps_the_latest_activity() {
    let mut batch = PendingUiBatch::new(1024);
    batch.push(UiEvent::Output(OutputEvent::Text("one".into())));
    batch.push(UiEvent::Activity(Activity::Thinking));
    batch.push(UiEvent::Output(OutputEvent::Text(" two".into())));
    batch.push(UiEvent::Activity(Activity::Working));

    let drained = batch.drain();
    assert_eq!(drained.text().as_bytes(), b"one two");
    assert_eq!(drained.outputs, vec![OutputEvent::Text("one two".into())]);
    assert_eq!(drained.activity, Some(Activity::Working));
    assert!(batch.is_empty());
}

#[test]
fn pending_batch_preserves_order_and_coalesces_same_output_kinds() {
    let mut batch = PendingUiBatch::new(1024);
    batch.push(UiEvent::Output(OutputEvent::Text("notice\n".into())));
    batch.push(UiEvent::Output(OutputEvent::StreamText("Com".into())));
    batch.push(UiEvent::Output(OutputEvent::StreamText("mitted".into())));

    let drained = batch.drain();
    assert_eq!(
        drained.outputs,
        vec![
            OutputEvent::Text("notice\n".into()),
            OutputEvent::StreamText("Committed".into()),
        ]
    );
    assert_eq!(drained.text(), "notice\nCommitted");
}

#[test]
fn pending_batch_keeps_the_latest_running_tool_update() {
    let mut batch = PendingUiBatch::new(1024);
    batch.push(UiEvent::RunningTool(Some("cargo test".into())));
    batch.push(UiEvent::RunningTool(None));
    batch.push(UiEvent::RunningTool(Some("cargo build".into())));

    let drained = batch.drain();
    assert_eq!(drained.running_tool, Some(Some("cargo build".to_string())));
    assert!(batch.drain().running_tool.is_none());
}

struct FloodSim<'a> {
    fragments: &'a mut VecDeque<String>,
    input: &'a mut VecDeque<UiAction>,
    batch: &'a mut PendingUiBatch,
    state: &'a mut InteractiveState,
    output: &'a mut String,
    frame: &'a mut usize,
    since_frame: &'a mut usize,
    input_visible_at: &'a mut Option<usize>,
}

impl FloodSim<'_> {
    fn step(&mut self) {
        if *self.since_frame == 64 || self.fragments.is_empty() {
            self.output.push_str(&self.batch.drain().text());
            *self.frame += 1;
            *self.since_frame = 0;
            return;
        }
        if let Some(action) = self.input.pop_front() {
            self.state.apply(action);
            self.input_visible_at.get_or_insert(*self.frame);
            return;
        }
        let fragment = self.fragments.pop_front().unwrap();
        if matches!(
            self.batch.push(UiEvent::Output(OutputEvent::Text(fragment))),
            BatchDecision::Flush(_)
        ) {
            self.output.push_str(&self.batch.drain().text());
        }
        *self.since_frame += 1;
    }
}

fn simulate_streaming_flood() -> (String, usize, String) {
    let mut fragments = (0..10_000).map(|index| format!("{index:05}|")).collect::<VecDeque<_>>();
    let mut input = VecDeque::from([UiAction::Insert('r'), UiAction::Insert('h'), UiAction::Insert('o')]);
    let mut state = InteractiveState::default();
    let mut batch = PendingUiBatch::new(4 * 1024);
    let mut output = String::new();
    let (mut frame, mut input_visible_at, mut since_frame) = (0, None, 0);

    while !fragments.is_empty() || !input.is_empty() || !batch.is_empty() {
        let mut sim = FloodSim {
            fragments: &mut fragments,
            input: &mut input,
            batch: &mut batch,
            state: &mut state,
            output: &mut output,
            frame: &mut frame,
            since_frame: &mut since_frame,
            input_visible_at: &mut input_visible_at,
        };
        sim.step();
    }
    (state.editor().text().to_string(), input_visible_at.unwrap(), output)
}

#[test]
fn streaming_flood_preserves_output_and_applies_input_within_two_frames() {
    let expected = (0..10_000).map(|i| format!("{i:05}|")).collect::<String>();
    let (text, visible_frame, output) = simulate_streaming_flood();
    assert_eq!(text, "rho");
    assert!(visible_frame <= 2);
    assert_eq!(output.as_bytes(), expected.as_bytes());
}

#[test]
fn pending_batch_newline_and_size_barriers() {
    let mut newline = PendingUiBatch::new(1024);
    assert!(matches!(
        newline.push(UiEvent::Output(OutputEvent::Text("line\n".into()))),
        BatchDecision::Flush(FlushBarrier::Newline)
    ));

    let mut size = PendingUiBatch::new(4);
    assert!(matches!(
        size.push(UiEvent::Output(OutputEvent::Text("1234".into()))),
        BatchDecision::Flush(FlushBarrier::Size)
    ));
}

#[tokio::test]
async fn pending_batch_interaction_barriers() {
    let mut size = PendingUiBatch::new(4);
    let (ui, mut events) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Modal".into(),
            body: String::new(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: Default::default(),
        })
        .await
    });
    let event = events.recv().await.unwrap();
    assert!(matches!(
        size.push(event),
        BatchDecision::Barrier(FlushBarrier::Interaction, UiEvent::Interaction { .. })
    ));
    drop(size);
    assert!(matches!(request.await.unwrap(), Err(UiPortError::Closed)));
}

#[tokio::test]
async fn channel_preserves_output_and_activity_order() {
    let (ui, mut receiver) = InteractiveUi::channel();
    ui.output(OutputEvent::Text("first".into())).unwrap();
    ui.set_activity(Activity::Thinking).unwrap();
    ui.output(OutputEvent::Text("second".into())).unwrap();

    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Output(OutputEvent::Text(text))) if text == "first"
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Activity(Activity::Thinking))
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Output(OutputEvent::Text(text))) if text == "second"
    ));
}

#[tokio::test]
async fn interaction_response_resolves_the_request() {
    let (ui, mut receiver) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Approval".into(),
            body: "Allow?".into(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: Default::default(),
        })
        .await
    });
    let Some(UiEvent::Interaction { responder, .. }) = receiver.recv().await else {
        panic!("expected interaction request");
    };

    responder.respond(InteractionResponse::Selected(0)).unwrap();
    assert_eq!(request.await.unwrap().unwrap(), InteractionResponse::Selected(0));
}

#[tokio::test]
async fn dropping_responder_reports_a_closed_request() {
    let (ui, mut receiver) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Question".into(),
            body: String::new(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: true,
            initial_text: None,
            option_layout: Default::default(),
        })
        .await
    });
    let Some(UiEvent::Interaction { responder, .. }) = receiver.recv().await else {
        panic!("expected interaction request");
    };
    drop(responder);

    assert!(matches!(request.await.unwrap(), Err(UiPortError::Closed)));
}

#[test]
fn tool_end_and_dismiss_interaction_send_their_events_in_order() {
    let (ui, mut receiver) = InteractiveUi::channel();
    ui.tool_end().unwrap();
    ui.dismiss_interaction().unwrap();

    assert!(matches!(receiver.try_recv(), Ok(UiEvent::ToolEnd)));
    assert!(matches!(receiver.try_recv(), Ok(UiEvent::DismissInteraction)));
}

#[test]
fn tool_end_and_dismiss_interaction_report_a_closed_channel() {
    let (ui, receiver) = InteractiveUi::channel();
    drop(receiver);

    assert!(matches!(ui.tool_end(), Err(UiPortError::Closed)));
    assert!(matches!(ui.dismiss_interaction(), Err(UiPortError::Closed)));
}

fn chunk_at(batch: &mut PendingUiBatch, text: &str, at: Instant) {
    batch.push_at(UiEvent::ToolChunk { chunk: text.into() }, at);
}

fn burst_after_a_pause(base: Instant) -> PendingUiBatch {
    let mut batch = PendingUiBatch::new(1024);
    chunk_at(&mut batch, "first\n", base);
    batch.drain_at(base);
    let burst: String = (1..=20).map(|i| format!("row {i}\n")).collect();
    chunk_at(&mut batch, &burst, base + std::time::Duration::from_millis(400));
    batch
}

#[test]
fn a_burst_of_tool_output_is_released_a_few_lines_per_frame() {
    let base = Instant::now();
    let mut batch = burst_after_a_pause(base);
    let mut per_frame = Vec::new();
    for frame in 1..=3u64 {
        let drained = batch.drain_at(base + std::time::Duration::from_millis(400 + frame * 16));
        per_frame.push(drained.tool_chunks.iter().map(|c| c.lines().count()).sum::<usize>());
    }
    assert!(per_frame.iter().all(|&n| n <= 2), "{per_frame:?}");
    assert!(per_frame.iter().sum::<usize>() >= 1, "{per_frame:?}");
    assert!(
        !batch.is_empty(),
        "the rest is still pending so the next frame flushes it"
    );
}

fn text_at(batch: &mut PendingUiBatch, text: &str, ms: u64, base: Instant) {
    batch.push_at(
        UiEvent::Output(OutputEvent::StreamText(text.into())),
        base + std::time::Duration::from_millis(ms),
    );
}

fn streamed(drained: &PendingUiDrain) -> String {
    drained.text()
}

#[test]
fn a_burst_of_streamed_text_is_released_a_few_words_per_frame_in_order() {
    let base = Instant::now();
    let mut batch = PendingUiBatch::new(4096);
    text_at(&mut batch, "Hello ", 0, base);
    batch.drain_at(base);
    let sentence = "the quick brown fox jumps over the lazy dog and keeps running far away from home ";
    text_at(&mut batch, sentence, 400, base);

    let mut shown = String::new();
    let mut frames = 0;
    while !batch.is_empty() && frames < 80 {
        frames += 1;
        let drained = batch.drain_at(base + std::time::Duration::from_millis(400 + frames * 16));
        let piece = streamed(&drained);
        assert!(piece.split_whitespace().count() <= 3, "{piece:?}");
        shown.push_str(&piece);
    }
    assert_eq!(shown, sentence);
    assert!(frames >= 15, "spread over only {frames} frames");
}

#[test]
fn a_notice_never_overtakes_streamed_text_that_came_before_it() {
    let base = Instant::now();
    let mut batch = PendingUiBatch::new(4096);
    text_at(&mut batch, "one ", 0, base);
    batch.drain_at(base);
    text_at(&mut batch, "two three four five ", 400, base);
    batch.push_at(
        UiEvent::Output(OutputEvent::Text("[notice]\n".into())),
        base + std::time::Duration::from_millis(405),
    );
    text_at(&mut batch, "six ", 410, base);

    let drained = batch.drain_at(base + std::time::Duration::from_millis(416));
    let text = drained.text();
    assert!(text.starts_with("two three four five [notice]\n"), "{text:?}");
    assert!(!text.contains("six") || text.ends_with("six "), "{text:?}");
}

#[test]
fn a_tool_card_shows_the_streamed_text_before_it_in_the_same_drain() {
    let base = Instant::now();
    let mut batch = PendingUiBatch::new(4096);
    text_at(&mut batch, "one ", 0, base);
    batch.drain_at(base);
    text_at(&mut batch, "two three four five six seven ", 400, base);
    let request = ToolStartRequest {
        name: "write".into(),
        args_summary: String::new(),
        arguments: serde_json::json!({}),
    };
    batch.push_at(
        UiEvent::ToolStart(request),
        base + std::time::Duration::from_millis(405),
    );
    let drained = batch.drain_at(base + std::time::Duration::from_millis(416));
    assert_eq!(drained.text(), "two three four five six seven ");
    assert!(batch.is_empty());
}

#[test]
fn a_tool_end_shows_the_whole_backlog_in_the_same_drain() {
    let base = Instant::now();
    let mut batch = burst_after_a_pause(base);
    batch.push_at(UiEvent::ToolEnd, base + std::time::Duration::from_millis(416));
    let drained = batch.drain_at(base + std::time::Duration::from_millis(416));
    assert_eq!(drained.tool_chunks.concat().lines().count(), 20);
    assert!(drained.tool_end);
    assert!(batch.is_empty());
}

#[test]
fn a_new_tool_card_drops_output_still_waiting_for_the_old_one() {
    let base = Instant::now();
    let mut batch = burst_after_a_pause(base);
    let request = ToolStartRequest {
        name: "write".into(),
        args_summary: "a.txt".into(),
        arguments: serde_json::json!({"path": "a.txt"}),
    };
    batch.push_at(
        UiEvent::ToolStart(request),
        base + std::time::Duration::from_millis(416),
    );
    let drained = batch.drain_at(base + std::time::Duration::from_millis(416));
    assert!(drained.tool_chunks.is_empty());
    assert!(drained.tool_start.is_some());
}

fn ends_inside_an_escape(text: &str) -> bool {
    text.rfind('\x1b').is_some_and(|start| {
        let tail = &text[start..];
        !tail.chars().skip(1).any(|c| c.is_ascii_alphabetic() && c != '[')
    })
}

#[test]
fn styled_thinking_text_is_identical_when_paced_and_never_cut_inside_an_escape() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = crate::ui::TerminalRenderer::with_ui(ui);
    renderer.set_width(60);
    let thinking = "## Plan\n\nI should read the **config** first, then check `parse_args` and the \
        [docs](https://example.com/a/b) before changing anything.\n\n- first item with *emphasis*\n\
        - second item\n\n```rust\nfn main() {}\n```\n\nThat covers it.\n";
    for token in thinking.split_inclusive(' ') {
        renderer.print_thinking_token(token);
    }
    renderer.flush();

    let mut unpaced = String::new();
    let mut batch = PendingUiBatch::new(1 << 20);
    let base = Instant::now();
    let mut ms = 0u64;
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Output(OutputEvent::StreamText(text) | OutputEvent::Text(text)) = &event {
            unpaced.push_str(text);
            ms += 150;
        }
        batch.push_at(event, base + std::time::Duration::from_millis(ms));
    }
    assert!(unpaced.contains("\x1b["), "the thinking text is styled: {unpaced:?}");

    let mut paced = String::new();
    let mut frame = 0u64;
    while !batch.is_empty() && frame < 4000 {
        frame += 1;
        let piece = batch
            .drain_at(base + std::time::Duration::from_millis(ms + frame * 16))
            .text();
        assert!(!ends_inside_an_escape(&piece), "cut inside an escape: {piece:?}");
        paced.push_str(&piece);
    }
    assert_eq!(paced, unpaced);
}
