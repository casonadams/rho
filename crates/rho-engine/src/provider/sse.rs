//! Shared SSE line framing, stream unfolding, and completion aggregation for
//! provider streaming implementations (OpenAI-compatible, Claude, Antigravity, ChatGPT).

use futures::{Stream, StreamExt, stream};
use rho_harness_core::error::AppError;
use std::pin::Pin;
use std::time::Duration;

use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::{ModelCompletionResponse, ModelStreamEvent};

pub type SseEvents = Vec<Result<ModelStreamEvent, AppError>>;

pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

pub const DONE_SENTINEL: &str = "[DONE]";

pub const MAX_SSE_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Splits SSE bytes into lines. A partial line longer than `max_line` is dropped through its end and
/// reported once by `take_overflow`, so a stream without newlines cannot grow the buffer without bound.
pub struct SseLineDecoder {
    buffer: Vec<u8>,
    scanned: usize,
    max_line: usize,
    discarding: bool,
    overflowed: bool,
}

impl Default for SseLineDecoder {
    fn default() -> Self {
        Self::with_max_line(MAX_SSE_LINE_BYTES)
    }
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_line(max_line: usize) -> Self {
        Self {
            buffer: Vec::new(),
            scanned: 0,
            max_line,
            discarding: false,
            overflowed: false,
        }
    }

    /// An error when a line went over the limit since the last call.
    pub fn take_overflow(&mut self) -> Option<AppError> {
        std::mem::take(&mut self.overflowed)
            .then(|| AppError::Network(format!("stream sent a line over {} bytes", self.max_line)))
    }

    /// `bytes` past the rest of an oversized line, or `None` while that line still has not ended.
    fn skip_oversized<'a>(&mut self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        if !self.discarding {
            return Some(bytes);
        }
        let end = memchr::memchr(b'\n', bytes)?;
        self.discarding = false;
        Some(&bytes[end + 1..])
    }

    pub fn decode_raw_lines(&mut self, bytes: &[u8]) -> Vec<String> {
        let Some(bytes) = self.skip_oversized(bytes) else {
            return Vec::new();
        };
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut cursor = 0;
        let mut scan_from = self.scanned;

        while let Some(rel) = memchr::memchr(b'\n', &self.buffer[scan_from..]) {
            let line_end = scan_from + rel;
            let line = String::from_utf8_lossy(&self.buffer[cursor..line_end]);
            lines.push(line.trim_end_matches('\r').to_string());
            cursor = line_end + 1;
            scan_from = cursor;
        }

        if cursor > 0 {
            self.buffer.drain(..cursor);
        }
        if self.buffer.len() > self.max_line {
            self.buffer.clear();
            self.discarding = true;
            self.overflowed = true;
        }
        self.scanned = self.buffer.len();
        lines
    }

    /// Feed byte chunks and extract ready `data:` payloads.
    pub fn decode_lines(&mut self, bytes: &[u8]) -> Vec<String> {
        self.decode_payloads(bytes)
            .into_iter()
            .filter(|payload| payload != DONE_SENTINEL)
            .collect()
    }

    /// Like `decode_lines`, but keeps the `[DONE]` sentinel for parsers whose wire format ends with it.
    pub fn decode_payloads(&mut self, bytes: &[u8]) -> Vec<String> {
        self.decode_raw_lines(bytes)
            .iter()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|payload| !payload.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Provider-specific incremental SSE parser that transforms wire bytes into domain stream events.
pub trait SseStreamParser: Send + 'static {
    fn feed(&mut self, bytes: &[u8]) -> SseEvents;

    /// Whether the wire format's terminal event (or an in-band error) arrived, so reading can stop.
    fn is_done(&self) -> bool;

    /// Events to emit when the body ends: a network error when it closed before its terminal event.
    fn finish(&mut self) -> SseEvents {
        if self.is_done() {
            return Vec::new();
        }
        vec![Err(AppError::Network(
            "provider stream closed before its terminal event".to_string(),
        ))]
    }
}

type StreamState<S, P> = (S, P, bool);

async fn next_stream_batch<B, S, P>(
    (mut bytes, mut parser, finished): StreamState<S, P>,
) -> Option<(SseEvents, StreamState<S, P>)>
where
    B: AsRef<[u8]>,
    S: Stream<Item = reqwest::Result<B>> + Unpin,
    P: SseStreamParser,
{
    if finished {
        return None;
    }
    loop {
        let chunk = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, bytes.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => {
                let events = parser.finish();
                return if events.is_empty() {
                    None
                } else {
                    Some((events, (bytes, parser, true)))
                };
            }
            Err(_) => {
                let err = AppError::Network(format!(
                    "provider stream went idle for {}s without sending data",
                    STREAM_IDLE_TIMEOUT.as_secs()
                ));
                return Some((vec![Err(err)], (bytes, parser, true)));
            }
        };
        match chunk {
            Ok(data) => {
                let events = parser.feed(data.as_ref());
                let finished = parser.is_done();
                if !events.is_empty() || finished {
                    return Some((events, (bytes, parser, finished)));
                }
            }
            Err(e) => {
                let err = AppError::Network(format!("Stream transport failed: {e}"));
                return Some((vec![Err(err)], (bytes, parser, true)));
            }
        }
    }
}

/// Unfold a provider's HTTP body into a pinned stream of domain events.
pub fn unfold_sse_stream<S, B, P>(
    bytes: S,
    parser: P,
) -> Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>
where
    S: Stream<Item = reqwest::Result<B>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send + 'static,
    P: SseStreamParser,
{
    let events = stream::unfold((bytes, parser, false), next_stream_batch)
        .map(stream::iter)
        .flatten();
    Box::pin(events)
}

/// Map a provider's opened HTTP response (or its `(status, body)` failure) into an event stream.
pub fn open_sse_stream<P: SseStreamParser>(
    opened: crate::provider::error::HttpResult,
    friendly_error: fn(Option<u16>, &str) -> String,
    parser: P,
) -> Result<crate::provider::adapter::ModelEventStream, AppError> {
    let response = opened.map_err(|failure| failure.into_app_error(friendly_error))?;
    Ok(unfold_sse_stream(response.bytes_stream(), parser))
}

/// Defines `feed` for a line-oriented SSE parser exposing a `decoder` field and `interpret_line`.
macro_rules! sse_line_feed {
    () => {
        pub fn feed(&mut self, bytes: &[u8]) -> $crate::provider::sse::SseEvents {
            let lines = self.decoder.decode_lines(bytes);
            let mut events = Vec::new();
            for line in lines {
                self.interpret_line(&line, &mut events);
            }
            events.extend(self.decoder.take_overflow().map(Err));
            events
        }
    };
}
pub(crate) use sse_line_feed;

/// Streams each payload as its own `data:` chunk through `parser` and collects every event.
#[cfg(test)]
pub(crate) async fn collect_sse_payloads<P: SseStreamParser>(payloads: &[&str], parser: P) -> SseEvents {
    let chunks: Vec<reqwest::Result<Vec<u8>>> = payloads
        .iter()
        .map(|payload| Ok(format!("data: {payload}\n\n").into_bytes()))
        .collect();
    unfold_sse_stream(stream::iter(chunks), parser).collect().await
}

/// Aggregate a vector of streaming events into a single unary `ModelCompletionResponse`.
pub fn aggregate_stream_events(events: SseEvents) -> Result<ModelCompletionResponse, AppError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = StructuralUsage::default();
    let mut finish_reason = None;

    for event in events {
        match event? {
            ModelStreamEvent::Text(t) => content.push_str(&t),
            ModelStreamEvent::Reasoning(_)
            | ModelStreamEvent::ReasoningBlock(_)
            | ModelStreamEvent::ToolInputDelta { .. } => {}
            ModelStreamEvent::ToolCall(call) => tool_calls.push(call),
            ModelStreamEvent::Usage(u) => usage = usage.merge(&u),
            ModelStreamEvent::Finish(f) => finish_reason = Some(f),
        }
    }

    Ok(ModelCompletionResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::model::{ToolCall, ToolFunction};

    #[test]
    fn sse_line_decoder_extracts_clean_lines_across_chunks() {
        let mut decoder = SseLineDecoder::new();

        let lines1 = decoder.decode_lines(b"data: {\"a\": 1}\r\ndata: ");
        assert_eq!(lines1, vec!["{\"a\": 1}"]);

        let lines2 = decoder.decode_lines(b"{\"b\": 2}\n\ndata: [DONE]\n");
        assert_eq!(lines2, vec!["{\"b\": 2}"]);
    }

    #[test]
    fn aggregate_stream_events_merges_text_and_reasoning() {
        let events = vec![
            Ok(ModelStreamEvent::Text("Hello ".to_string())),
            Ok(ModelStreamEvent::Reasoning("thinking".to_string())),
            Ok(ModelStreamEvent::Text("world!".to_string())),
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            })),
        ];

        let response = aggregate_stream_events(events).unwrap();
        assert_eq!(response.content, "Hello world!");
        assert_eq!(response.usage.total_tokens, 15);
    }

    #[test]
    fn aggregate_stream_events_attaches_tool_call_and_usage() {
        let events = vec![
            Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("bash", serde_json::json!({"command": "ls"})),
            ))),
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            })),
        ];

        let response = aggregate_stream_events(events).unwrap();
        assert_eq!(response.usage.total_tokens, 15);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].function.name, "bash");
    }

    #[derive(Default)]
    struct TerminalParser {
        done: bool,
    }

    impl SseStreamParser for TerminalParser {
        fn feed(&mut self, bytes: &[u8]) -> SseEvents {
            let event = match bytes {
                b"usage" => ModelStreamEvent::Usage(StructuralUsage::default()),
                b"end" => {
                    self.done = true;
                    return Vec::new();
                }
                other => ModelStreamEvent::Text(String::from_utf8_lossy(other).into_owned()),
            };
            vec![Ok(event)]
        }

        fn is_done(&self) -> bool {
            self.done
        }
    }

    async fn run_terminal_parser(chunks: &[&str]) -> SseEvents {
        let body: Vec<reqwest::Result<Vec<u8>>> = chunks.iter().map(|c| Ok(c.as_bytes().to_vec())).collect();
        unfold_sse_stream(stream::iter(body), TerminalParser::default())
            .collect()
            .await
    }

    #[tokio::test]
    async fn unfold_reads_past_usage_and_stops_once_the_parser_is_done() {
        let events = run_terminal_parser(&["usage", "text", "end", "ignored"]).await;
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(&events[1], Ok(ModelStreamEvent::Text(t)) if t == "text"));
    }

    #[tokio::test]
    async fn body_closing_before_the_parser_is_done_ends_with_a_network_error() {
        let events = run_terminal_parser(&["text"]).await;
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(&events[1], Err(AppError::Network(_))));
    }

    #[test]
    fn aggregate_stream_events_sums_usage_across_events() {
        let usage = |total| {
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 4,
                total_tokens: total,
                ..Default::default()
            }))
        };
        let response = aggregate_stream_events(vec![usage(10), usage(5)]).unwrap();
        assert_eq!((response.usage.input_tokens, response.usage.total_tokens), (8, 15));
    }

    #[test]
    fn sse_line_decoder_joins_line_split_over_many_chunks() {
        let mut decoder = SseLineDecoder::new();
        assert!(decoder.decode_raw_lines(b"data: a").is_empty());
        assert!(decoder.decode_raw_lines(b"bbb").is_empty());
        assert!(decoder.decode_raw_lines(b"c").is_empty());
        assert_eq!(decoder.decode_raw_lines(b"\nnext\n"), vec!["data: abbbc", "next"]);
    }

    #[test]
    fn sse_line_decoder_drops_an_oversized_line_and_reports_it_once() {
        let mut decoder = SseLineDecoder::with_max_line(4);
        assert_eq!(decoder.decode_raw_lines(b"ok\nabc"), vec!["ok"]);
        assert!(decoder.take_overflow().is_none());

        assert!(decoder.decode_raw_lines(b"defgh").is_empty());
        assert!(decoder.decode_raw_lines(b"ijk").is_empty());
        assert!(matches!(decoder.take_overflow(), Some(AppError::Network(_))));
        assert!(decoder.take_overflow().is_none());

        assert_eq!(decoder.decode_raw_lines(b"lm\nnext\n"), vec!["next"]);
    }

    #[test]
    fn sse_line_decoder_keeps_multibyte_characters_split_across_chunks() {
        let mut decoder = SseLineDecoder::new();
        let bytes = "data: \"é\"\n".as_bytes();
        let split = bytes.iter().position(|b| *b == 0xC3).unwrap() + 1;
        assert!(decoder.decode_raw_lines(&bytes[..split]).is_empty());
        assert_eq!(decoder.decode_raw_lines(&bytes[split..]), vec!["data: \"é\""]);
    }

    #[tokio::test(start_paused = true)]
    async fn unfold_sse_stream_fails_instead_of_hanging_when_body_goes_idle() {
        struct NoEvents;
        impl SseStreamParser for NoEvents {
            fn feed(&mut self, _bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>> {
                Vec::new()
            }

            fn is_done(&self) -> bool {
                false
            }
        }

        let idle_body = futures::stream::iter(vec![Ok::<_, reqwest::Error>(b"keepalive".to_vec())])
            .chain(futures::stream::pending());
        let mut events = unfold_sse_stream(idle_body, NoEvents);
        let first = tokio::time::timeout(Duration::from_secs(3600), events.next())
            .await
            .expect("idle stream must error, not hang");
        assert!(matches!(first, Some(Err(AppError::Network(msg))) if msg.contains("idle")));
    }
}
