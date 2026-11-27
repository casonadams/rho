use super::*;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::{ToolStreamPort, ToolStreamSink};
use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};
use rho_harness_core::session::SessionManager;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct CallLog(pub(crate) Mutex<Vec<String>>);

impl CallLog {
    fn push(&self, call: String) {
        self.0.lock().unwrap().push(call);
    }
}

impl ToolStreamSink for CallLog {
    fn tool_chunk(&self, chunk: String) {
        self.push(format!("chunk {chunk}"));
    }
}

/// An interactive presenter that logs the tool cards it is asked to show.
pub(crate) struct InteractivePresenter(pub(crate) Arc<CallLog>);

#[async_trait::async_trait]
impl Presenter for InteractivePresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, token: &str) {
        self.0.push(format!("token {token}"));
    }
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {
        self.0.push("flush".into());
    }
    fn has_interactive_ui(&self) -> bool {
        true
    }
    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, name: &str, arguments: &serde_json::Value) {
        self.0.push(format!("start {name} {arguments}"));
    }
    fn end_tool_run(&self) {
        self.0.push("end".into());
    }
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::new(Some(self.0.clone() as Arc<dyn ToolStreamSink>))
    }
}

fn extract_json_streaming_content(json: &str) -> Option<String> {
    let mut decoder = ContentDecoder::default();
    decoder.advance(json);
    decoder.body_start.map(|_| decoder.decoded)
}

/// Whether the write card shows its path yet, and how much content it has streamed.
fn write_preview(tracker: &StreamingToolTracker) -> Option<(bool, usize)> {
    match &tracker.preview {
        Some(Preview::Write(preview)) => Some((preview.path_shown, preview.streamed_content_len)),
        _ => None,
    }
}

fn mock_sink() -> (Arc<TerminalApprovalSink>, crate::test_support::TestDir) {
    sink_for(crate::engine::eval::presenter::presenter())
}

fn sink_for(presenter: Arc<dyn Presenter>) -> (Arc<TerminalApprovalSink>, crate::test_support::TestDir) {
    let temp_dir = crate::test_support::TestDir::new();
    let session = SessionManager::new(&temp_dir, None).unwrap();
    let sink = TerminalApprovalSink::new(
        &presenter,
        TerminalSinkConfig {
            model_label: "test-model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );
    (sink, temp_dir)
}

#[test]
fn extract_path_from_partial_json() {
    let json = r#"{"path": "src/main.rs", "content": "hello"#;
    assert_eq!(extract_json_string_field(json, "path").as_deref(), Some("src/main.rs"));

    let incomplete = r#"{"path": "src/main"#;
    assert_eq!(extract_json_string_field(incomplete, "path"), None);

    let escaped = r#"{"path": "foo/bar\"baz/test.rs"}"#;
    assert_eq!(
        extract_json_string_field(escaped, "path").as_deref(),
        Some("foo/bar\"baz/test.rs")
    );
}

#[test]
fn extract_streaming_content_progressively() {
    let chunk1 = r#"{"path": "test.txt", "content": "first line\n"#;
    assert_eq!(extract_json_streaming_content(chunk1).as_deref(), Some("first line\n"));

    let chunk2 = r#"{"path": "test.txt", "content": "first line\nsecond line"#;
    assert_eq!(
        extract_json_streaming_content(chunk2).as_deref(),
        Some("first line\nsecond line")
    );

    let chunk3 = r#"{"path": "test.txt", "content": "first line\nsecond line\n"}"#;
    assert_eq!(
        extract_json_streaming_content(chunk3).as_deref(),
        Some("first line\nsecond line\n")
    );
}

#[test]
fn extract_handles_all_escapes() {
    let json = r#"{"content": "quote:\" slash:\\ slash2:\/ backspace:\b ff:\f lf:\n cr:\r tab:\t esc:\u001b"}"#;
    assert_eq!(
        extract_json_streaming_content(json).as_deref(),
        Some("quote:\" slash:\\ slash2:/ backspace:\x08 ff:\x0c lf:\n cr:\r tab:\t esc:\x1b")
    );
}

#[test]
fn extract_trailing_escape_in_streaming_content() {
    let json = "{\"content\": \"hello\\";
    assert_eq!(extract_json_streaming_content(json).as_deref(), Some("hello"));

    let non_streaming = "{\"path\": \"hello\\";
    assert_eq!(extract_json_string_field(non_streaming, "path"), None);
}

#[test]
fn content_decoder_reads_the_content_serde_reads_at_every_split() {
    let full = "{\"path\": \"a.txt\", \"content\": \"é\\n\\\"q\\\" \\u001b[1m \\ud83d\\ude00 \\u00e9 tab\\t end\"}";
    let expected = serde_json::from_str::<Value>(full).unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(expected.contains('\x1b') && expected.contains('\u{1f600}'));

    for split in (0..=full.len()).filter(|&i| full.is_char_boundary(i)) {
        let mut decoder = ContentDecoder::default();
        decoder.advance(&full[..split]);
        assert!(
            expected.starts_with(&decoder.decoded),
            "split at {split}: {:?}",
            decoder.decoded
        );
        decoder.advance(full);
        assert_eq!(decoder.decoded, expected, "split at {split}");
    }
}

#[test]
fn content_decoder_holds_escape_split_across_deltas() {
    let mut decoder = ContentDecoder::default();
    decoder.advance("{\"content\": \"hi\\");
    assert_eq!(decoder.decoded, "hi");
    decoder.advance("{\"content\": \"hi\\n");
    assert_eq!(decoder.decoded, "hi\n");
}

#[test]
fn extract_missing_or_malformed_fields() {
    assert_eq!(extract_json_string_field("{}", "path"), None);
    assert_eq!(extract_json_string_field("{\"path\"", "path"), None);
    assert_eq!(extract_json_string_field("{\"path\": 123}", "path"), None);

    assert_eq!(extract_json_streaming_content("{}"), None);
    assert_eq!(extract_json_streaming_content("{\"content\""), None);
    assert_eq!(extract_json_streaming_content("{\"content\": false}"), None);
}

#[test]
fn streaming_tool_tracker_write_lifecycle() {
    let (sink, _dir) = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta("call-1", "write", r#"{"path": "foo.txt", "content": "hello "#, &sink);
    assert_eq!(write_preview(&tracker), Some((true, 6)));

    tracker.handle_delta("call-1", "write", r#"world"}"#, &sink);
    assert_eq!(write_preview(&tracker), Some((true, 11)));

    tracker.handle_delta("call-1", "write", "", &sink);
    assert_eq!(write_preview(&tracker), Some((true, 11)));

    tracker.handle_delta("call-2", "write", r#"{"path": "#, &sink);
    assert_eq!(write_preview(&tracker), Some((false, 0)), "a new call id starts over");
}

#[test]
fn streaming_tool_tracker_streams_content_written_before_the_path() {
    let (sink, _dir) = mock_sink();
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta("call-1", "write", r#"{"content": "data", "#, &sink);
    assert_eq!(write_preview(&tracker), Some((false, 4)));

    tracker.handle_delta("call-1", "write", r#""file_path": "bar.txt"}"#, &sink);
    assert_eq!(
        write_preview(&tracker),
        Some((true, 4)),
        "the card reopens with the path and replays the content so far"
    );
}

#[test]
fn streaming_tool_tracker_opens_with_the_path_and_streams_text_when_the_path_comes_first() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta("call-1", "write", r#"{"path": "a.rs", "text": "fn "#, &sink);
    tracker.handle_delta("call-1", "write", r#"main"}"#, &sink);

    let calls = log.0.lock().unwrap().clone();
    assert_eq!(
        calls,
        ["flush", r#"start write {"path":"a.rs"}"#, "chunk fn ", "chunk main"]
    );
}

#[test]
fn streaming_tool_tracker_only_previews_write_and_edit_calls() {
    let (sink, _dir) = mock_sink();
    let mut tracker = StreamingToolTracker::default();
    tracker.handle_delta("call-1", "bash", r#"{"command": "echo hi"}"#, &sink);
    assert!(tracker.preview.is_none());
}

#[test]
fn an_edit_card_opens_at_once_and_grows_line_by_line_as_its_arguments_stream() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();

    for chunk in [
        "",
        r#"{"path": "a.rs", "edits": [{"oldText": "fn a() {\n"#,
        "    old",
        r#"();\n}", "newText": "fn a() {}"}]}"#,
    ] {
        tracker.handle_delta("call-1", "edit", chunk, &sink);
    }

    let calls = log.0.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            "flush",
            "start edit {}",
            r#"start edit {"path":"a.rs","edits":[{"oldText":"fn a() {\n"}]}"#,
            r#"start edit {"path":"a.rs","edits":[{"oldText":"fn a() {\n    old();\n}","newText":"fn a() {}"}]}"#,
        ],
        "the fragment without a line end waits for one"
    );
}

#[test]
fn an_edit_card_names_its_file_only_once_the_whole_path_has_streamed() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();

    tracker.handle_delta("call-1", "edit", r#"{"edits": [], "path": "src/ma"#, &sink);
    tracker.handle_delta("call-1", "edit", r#"in.rs"}"#, &sink);

    let calls = log.0.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            "flush",
            r#"start edit {"edits":[]}"#,
            r#"start edit {"edits":[],"path":"src/main.rs"}"#
        ]
    );
}

#[test]
fn a_previewed_call_is_shown_with_its_final_arguments_when_it_starts() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();
    tracker.handle_delta("call-1", "edit", r#"{"path": "a.rs", "edits": [{"oldText": "x"#, &sink);

    let args = serde_json::json!({ "path": "a.rs", "edits": [{ "oldText": "x", "newText": "y" }] });
    sink.tool_start("edit", &args);

    let calls = log.0.lock().unwrap().clone();
    assert_eq!(calls.last(), Some(&format!("start edit {args}")));
}

#[test]
fn a_call_is_planned_without_the_card_of_a_call_streamed_after_it() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();
    tracker.handle_delta("call-1", "edit", r#"{"path": "a.rs", "edits": []}"#, &sink);
    tracker.handle_delta("call-2", "edit", r#"{"path": "b.rs", "edits": []}"#, &sink);
    log.0.lock().unwrap().clear();

    let first = serde_json::json!({ "path": "a.rs", "edits": [] });
    sink.focus_tool_preview("call-1");
    sink.tool_start("edit", &first);

    let calls = log.0.lock().unwrap().clone();
    assert_eq!(
        calls,
        ["end".to_string(), "flush".into(), format!("start edit {first}")]
    );
}

#[test]
fn the_card_of_the_call_being_planned_stays_up() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();
    tracker.handle_delta("call-1", "edit", r#"{"path": "a.rs", "edits": []}"#, &sink);
    log.0.lock().unwrap().clear();

    sink.focus_tool_preview("call-1");

    assert!(log.0.lock().unwrap().is_empty());
}

#[test]
fn a_tool_call_shows_the_reply_so_far_and_previews_write_content_as_it_streams() {
    let log = Arc::new(CallLog::default());
    let (sink, _dir) = sink_for(Arc::new(InteractivePresenter(log.clone())));
    let mut tracker = StreamingToolTracker::default();

    sink.emit_text("Writing the notes now");
    tracker.handle_delta("call-1", "write", "", &sink);
    tracker.handle_delta("call-1", "write", r#"{"content": "alpha\n"#, &sink);
    tracker.handle_delta("call-1", "write", r#"beta\n", "path": "notes.txt"}"#, &sink);
    sink.cancel_tool_preview();
    sink.cancel_tool_preview();

    let calls = log.0.lock().unwrap().clone();
    let after_flush: Vec<&str> = calls
        .iter()
        .skip_while(|call| *call != "flush")
        .skip(1)
        .map(String::as_str)
        .collect();
    assert_eq!(
        after_flush,
        [
            "start write {}",
            "chunk alpha\n",
            r#"start write {"path":"notes.txt"}"#,
            "chunk alpha\nbeta\n",
            "end",
        ]
    );
}
