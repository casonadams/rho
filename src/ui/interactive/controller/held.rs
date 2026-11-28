//! The main agent's output held while another agent is shown, merged as it arrives so returning
//! applies it at once instead of one drain per frame.

use crate::ui::interactive::{OutputEvent, PendingUiDrain};

/// How much of the running tool's streamed output is held; its card shows only the latest anyway.
pub const MAX_HELD_TOOL_BYTES: usize = 100 * 1024;

/// Adds `next` to `held`: dropped when empty, merged into the last held drain when applying the two
/// together shows the same as applying them in turn, otherwise kept after it.
pub fn hold(held: &mut Vec<PendingUiDrain>, next: PendingUiDrain) {
    if next == PendingUiDrain::default() {
        return;
    }
    match held.last_mut() {
        Some(last) if merges(last, &next) => merge(last, next),
        _ => held.push(next),
    }
    if let Some(last) = held.last_mut() {
        trim_tool_chunks(&mut last.tool_chunks);
    }
}

/// A drain applies its tool updates, then its output, then its transcript items, so `next` merges
/// into `last` only when nothing in `last` must come before what `next` does first.
fn merges(last: &PendingUiDrain, next: &PendingUiDrain) -> bool {
    let last_settled = !last.transcript_items.is_empty() || last.trailing_recap.is_some() || last.tool_end;
    let next_starts_tool = next.tool_start.is_some();
    let last_has_tool = last.tool_start.is_some() || !last.tool_chunks.is_empty();
    !last_settled && !(next_starts_tool && (last_has_tool || !last.outputs.is_empty()))
}

fn merge(last: &mut PendingUiDrain, next: PendingUiDrain) {
    for output in next.outputs {
        push_output(&mut last.outputs, output);
    }
    if next.activity.is_some() {
        last.activity = next.activity;
    }
    if next.running_tool.is_some() {
        last.running_tool = next.running_tool;
    }
    if next.tool_start.is_some() {
        last.tool_start = next.tool_start;
    }
    last.tool_chunks.extend(next.tool_chunks);
    last.tool_end = next.tool_end;
    if next.system_message.is_some() {
        last.system_message = next.system_message;
    }
    last.transcript_items = next.transcript_items;
    last.trailing_recap = next.trailing_recap;
    last.a2ui_messages.extend(next.a2ui_messages);
}

/// Appends `output`, joining it to the last output of the same kind.
fn push_output(outputs: &mut Vec<OutputEvent>, output: OutputEvent) {
    match (outputs.last_mut(), output) {
        (Some(OutputEvent::Text(last)), OutputEvent::Text(text))
        | (Some(OutputEvent::StreamText(last)), OutputEvent::StreamText(text)) => last.push_str(&text),
        (_, output) => outputs.push(output),
    }
}

/// Keeps the last `MAX_HELD_TOOL_BYTES` of `chunks`.
fn trim_tool_chunks(chunks: &mut Vec<String>) {
    let mut total: usize = chunks.iter().map(String::len).sum();
    let mut whole = 0;
    while chunks
        .get(whole)
        .is_some_and(|c| total - c.len() >= MAX_HELD_TOOL_BYTES)
    {
        total -= chunks[whole].len();
        whole += 1;
    }
    chunks.drain(..whole);
    if total > MAX_HELD_TOOL_BYTES
        && let Some(first) = chunks.first_mut()
    {
        let cut = first.ceil_char_boundary(total - MAX_HELD_TOOL_BYTES);
        first.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{Activity, ToolStartRequest, TranscriptItem};
    use test_case::test_case;

    fn output(text: &str) -> PendingUiDrain {
        PendingUiDrain {
            outputs: vec![OutputEvent::StreamText(text.into())],
            ..PendingUiDrain::default()
        }
    }

    fn tool(name: &str) -> PendingUiDrain {
        PendingUiDrain {
            tool_start: Some(ToolStartRequest {
                name: name.into(),
                arguments: serde_json::json!({}),
                file_change: None,
            }),
            ..PendingUiDrain::default()
        }
    }

    fn chunk(text: &str) -> PendingUiDrain {
        PendingUiDrain {
            tool_chunks: vec![text.into()],
            ..PendingUiDrain::default()
        }
    }

    fn notice(text: &str) -> PendingUiDrain {
        PendingUiDrain {
            transcript_items: vec![TranscriptItem::Notice(text.into())],
            ..PendingUiDrain::default()
        }
    }

    fn held(drains: Vec<PendingUiDrain>) -> Vec<PendingUiDrain> {
        let mut held = Vec::new();
        for drain in drains {
            hold(&mut held, drain);
        }
        held
    }

    #[test]
    fn empty_drains_are_not_held() {
        assert!(held((0..3).map(|_| PendingUiDrain::default()).collect()).is_empty());
    }

    #[test]
    fn streamed_output_is_joined_into_one_drain() {
        assert_eq!(held(vec![output("a"), output("b"), output("c")]), [output("abc")]);
    }

    #[test]
    fn latest_status_wins() {
        let status = |activity, running: &str| PendingUiDrain {
            activity: Some(activity),
            running_tool: Some(Some(running.into())),
            ..PendingUiDrain::default()
        };
        let merged = held(vec![
            status(Activity::Thinking, "read"),
            status(Activity::Working, "bash"),
        ]);
        assert_eq!(merged, [status(Activity::Working, "bash")]);
    }

    #[test]
    fn a_tools_start_and_chunks_merge_with_its_end_and_transcript() {
        let end = PendingUiDrain {
            tool_end: true,
            transcript_items: vec![TranscriptItem::Notice("done".into())],
            ..PendingUiDrain::default()
        };
        let merged = held(vec![tool("bash"), chunk("one "), chunk("two"), end]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].tool_start.as_ref().map(|t| t.name.as_str()), Some("bash"));
        assert_eq!(merged[0].tool_chunks, ["one ", "two"]);
        assert!(merged[0].tool_end);
        assert_eq!(merged[0].transcript_items, [TranscriptItem::Notice("done".into())]);
    }

    #[test_case(|| vec![notice("first"), output("then")]; "output after a transcript item")]
    #[test_case(|| vec![tool("read"), tool("bash")]; "a second tool after the first")]
    #[test_case(|| vec![output("text"), tool("bash")]; "a tool after output")]
    fn drains_keep_their_order_where_merging_would_change_it(drains: fn() -> Vec<PendingUiDrain>) {
        assert_eq!(held(drains()), drains());
    }

    #[test]
    fn held_tool_chunks_keep_only_the_latest_bytes() {
        let big = "x".repeat(MAX_HELD_TOOL_BYTES / 2);
        let merged = held(vec![tool("bash"), chunk("old"), chunk(&big), chunk(&big), chunk("new")]);
        let chunks = &merged[0].tool_chunks;
        assert_eq!(chunks.iter().map(String::len).sum::<usize>(), MAX_HELD_TOOL_BYTES);
        assert!(!chunks.concat().contains("old"));
        assert!(chunks.concat().ends_with("new"));
    }
}
