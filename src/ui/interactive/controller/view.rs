//! Shows another agent in the terminal in place of the main agent. The main agent's transcript,
//! partial output, and running tool are parked, and output it produces meanwhile is held, so
//! returning replays its transcript and then everything it did while another agent was shown.

use super::TerminalController;
use super::ansi::terminal_newlines;
use super::backend::TerminalBackend;
use super::cache::target_slot;
use crate::ui::interactive::{PendingUiDrain, RunningTool, ToolStartRequest, TranscriptItem};
use crate::ui::render::stream::{StreamKind, StreamRenderer};
use crate::ui::theme::Theme;
use std::io;

/// What the shown agent looks like now: its settled transcript, the reply or thinking it is still
/// writing, and the tool it is running with the output streamed to it so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFrame {
    pub items: Vec<TranscriptItem>,
    /// The sequence number of the agent event each of `items` shows, ascending.
    pub item_seqs: Vec<usize>,
    pub streaming: Option<StreamingText>,
    pub running_tool: Option<ToolStartRequest>,
    pub running_output: String,
    /// How many bytes streamed to the running tool precede `running_output`, which keeps the latest.
    pub running_output_start: usize,
}

/// A reply or thinking still being written, as far as it has streamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingText {
    pub kind: StreamKind,
    pub text: String,
}

pub(crate) struct AgentView {
    agent: u64,
    parked: Parked,
    /// The reply or thinking being streamed to the terminal.
    stream: Option<LiveStream>,
    /// How much of the running tool's output the card already shows.
    tool_streamed: usize,
    /// The monitor version the shown frame was last synced at.
    synced_version: Option<u64>,
    /// The sequence number of the event behind each shown transcript item.
    shown_seqs: Vec<usize>,
}

struct Parked {
    transcript: Vec<TranscriptItem>,
    streamed_output: String,
    active_tool: Option<RunningTool>,
    held: Vec<PendingUiDrain>,
}

/// Streams the shown agent's reply or thinking through the renderer the main agent streams with.
struct LiveStream {
    renderer: StreamRenderer,
    shown: String,
}

impl LiveStream {
    fn new(kind: StreamKind, theme: &Theme) -> Self {
        Self {
            renderer: StreamRenderer::new(kind, theme),
            shown: String::new(),
        }
    }

    /// Whether `text` of `kind` carries on from what is shown; a settled text may drop the trailing
    /// whitespace it streamed with.
    fn continues(&self, kind: StreamKind, text: &str) -> bool {
        kind == self.renderer.kind() && text.starts_with(self.shown.trim_end())
    }

    /// Renders what `text` adds to what is shown.
    fn advance(&mut self, text: &str, width: usize, theme: &Theme) -> String {
        let added = text.get(self.shown.len()..).unwrap_or_default();
        let rendered = self.renderer.push(added, width, theme);
        self.shown = text.to_string();
        rendered
    }

    /// Renders the rest of the settled `text`, the part held back included.
    fn finish(mut self, text: &str, width: usize, theme: &Theme) -> String {
        let mut rendered = self.advance(text, width, theme);
        rendered.push_str(&self.renderer.finish(theme));
        rendered
    }
}

fn streamed_part(item: &TranscriptItem) -> Option<(StreamKind, &str)> {
    match item {
        TranscriptItem::AssistantText(text) => Some((StreamKind::Reply, text)),
        TranscriptItem::Thinking(text) => Some((StreamKind::Thinking, text)),
        _ => None,
    }
}

/// Whether the next frame carries on the stream on screen: its first new item settles it, or, with
/// no new items, it is still streaming.
fn stream_continues(
    stream: Option<&LiveStream>,
    new_items: &[TranscriptItem],
    streaming: Option<&StreamingText>,
) -> bool {
    let Some(stream) = stream else {
        return true;
    };
    let next = match new_items.first() {
        Some(item) => streamed_part(item),
        None => streaming.map(|streaming| (streaming.kind, streaming.text.as_str())),
    };
    next.is_some_and(|(kind, text)| stream.continues(kind, text))
}

/// How the shown items line up with `frame`'s: how many leading shown items show events the agent
/// has since dropped, and how many after those `frame` starts with unchanged.
fn aligned_prefix(shown: &[TranscriptItem], shown_seqs: &[usize], frame: &ViewFrame) -> (usize, usize) {
    let dropped = frame
        .item_seqs
        .first()
        .map_or(0, |first| shown_seqs.iter().take_while(|seq| *seq < first).count())
        .min(shown.len());
    let kept = shown[dropped..]
        .iter()
        .zip(&shown_seqs[dropped..])
        .zip(frame.items.iter().zip(&frame.item_seqs))
        .take_while(|(a, b)| a == b)
        .count();
    (dropped, kept)
}

fn same_tool(running: Option<&RunningTool>, request: Option<&ToolStartRequest>) -> bool {
    match (running, request) {
        (None, None) => true,
        (Some(tool), Some(request)) => tool.name == request.name && tool.arguments == request.arguments,
        _ => false,
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    /// The agent shown instead of the main agent, if any.
    pub fn viewed_agent(&self) -> Option<u64> {
        self.view.as_ref().map(|view| view.agent)
    }

    /// The monitor version the shown agent was last synced at; `None` until it is synced.
    pub fn view_synced_version(&self) -> Option<u64> {
        self.view.as_ref().and_then(|view| view.synced_version)
    }

    /// Records that the shown agent is synced with the monitor at `version`.
    pub fn set_view_synced_version(&mut self, version: u64) {
        if let Some(view) = self.view.as_mut() {
            view.synced_version = Some(version);
        }
    }

    /// Shows agent `agent` as `frame`, titled `title` in the footer. The main agent stays parked
    /// when switching from one agent to another.
    pub fn enter_agent_view(&mut self, agent: u64, title: String, frame: ViewFrame) -> io::Result<()> {
        let parked = match self.view.take() {
            Some(view) => view.parked,
            None => Parked {
                transcript: std::mem::take(&mut self.transcript),
                streamed_output: std::mem::take(&mut self.streamed_output),
                active_tool: self.state.active_tool().cloned(),
                held: Vec::new(),
            },
        };
        self.view = Some(AgentView {
            agent,
            parked,
            stream: None,
            tool_streamed: 0,
            synced_version: None,
            shown_seqs: Vec::new(),
        });
        self.state.footer_mut().viewing = Some(title);
        self.show_frame(frame)
    }

    /// Updates the footer title of the shown agent.
    pub fn set_view_title(&mut self, title: String) -> bool {
        let footer = self.state.footer_mut();
        if footer.viewing.as_deref() == Some(title.as_str()) {
            return false;
        }
        footer.viewing = Some(title);
        true
    }

    /// Replaces the screen with `frame`, replaying it from the top.
    fn show_frame(&mut self, frame: ViewFrame) -> io::Result<()> {
        self.transcript = frame.items;
        self.cache.clear();
        if let Some(view) = self.view.as_mut() {
            view.shown_seqs = frame.item_seqs;
        }
        let (stream, streamed) = match &frame.streaming {
            Some(streaming) => {
                let mut stream = LiveStream::new(streaming.kind, &self.theme);
                let streamed = stream.advance(&streaming.text, self.width, &self.theme);
                (Some(stream), streamed)
            }
            None => (None, String::new()),
        };
        self.streamed_output = terminal_newlines(&streamed);
        if let Some(view) = self.view.as_mut() {
            view.stream = stream;
        }
        let running = frame.running_tool.map(RunningTool::start);
        self.state.footer_mut().running_tool = running.as_ref().map(|tool| tool.name.clone());
        self.state.set_active_tool(running);
        if let Some(tool) = self.state.active_tool_mut() {
            tool.append_chunk(&frame.running_output);
        }
        if let Some(view) = self.view.as_mut() {
            view.tool_streamed = frame.running_output_start + frame.running_output.len();
        }
        self.replay_transcript()
    }

    /// Brings the shown agent up to `frame`: new items are written below the old ones and the reply
    /// or thinking keeps streaming; anything that changed in place is replayed. Returns whether it
    /// changed.
    pub fn sync_agent_view(&mut self, frame: ViewFrame) -> io::Result<bool> {
        let Some(view) = self.view.as_ref() else {
            return Ok(false);
        };
        let (dropped, kept) = aligned_prefix(&self.transcript, &view.shown_seqs, &frame);
        let continues = stream_continues(view.stream.as_ref(), &frame.items[kept..], frame.streaming.as_ref());
        if dropped + kept < self.transcript.len() || !continues {
            self.show_frame(frame)?;
            return Ok(true);
        }
        let mut changed = false;
        for (item, seq) in frame.items.into_iter().zip(frame.item_seqs).skip(kept) {
            if let Some(view) = self.view.as_mut() {
                view.shown_seqs.push(seq);
            }
            self.push_view_item(item)?;
            changed = true;
        }
        changed |= self.stream_view_text(frame.streaming)?;
        changed |= self.set_view_tool(frame.running_tool, &frame.running_output, frame.running_output_start)?;
        Ok(changed)
    }

    fn push_view_item(&mut self, item: TranscriptItem) -> io::Result<()> {
        let Some((kind, text)) = streamed_part(&item) else {
            return self.push_transcript_item(item).map(drop);
        };
        match self.view.as_mut().and_then(|view| view.stream.take()) {
            Some(stream) => {
                let rest = stream.finish(text, self.width, &self.theme);
                self.settle_stream(kind, &rest, item)
            }
            None => self.write_settled_text(item),
        }
    }

    /// Ends the stream on screen with `rest`, the last of the text it settled into as `item`, the way
    /// the main agent's turn ends a reply or thinking.
    fn settle_stream(&mut self, kind: StreamKind, rest: &str, item: TranscriptItem) -> io::Result<()> {
        if !rest.is_empty() {
            self.write_stream_output(rest)?;
        }
        self.push_transcript_item(item)?;
        if kind == StreamKind::Thinking {
            self.write_output("\n")?;
        }
        Ok(())
    }

    /// Writes a reply or thinking that settled before any of it streamed to the screen.
    fn write_settled_text(&mut self, item: TranscriptItem) -> io::Result<()> {
        let needs_prefix_newline = self.output.is_open();
        let rendered = self.render_detached(&item, self.width);
        self.cache.push(
            target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
            &rendered,
        );
        self.transcript.push(item);
        if !rendered.is_empty() {
            self.write_transcript_card(&rendered, needs_prefix_newline)?;
        }
        Ok(())
    }

    /// Streams what the reply or thinking added since the last sync. A new one starts behind a blank
    /// row unless it follows a settled text of its own kind, as in the main agent's turn.
    fn stream_view_text(&mut self, streaming: Option<StreamingText>) -> io::Result<bool> {
        let (Some(streaming), Some(view)) = (streaming, self.view.as_mut()) else {
            return Ok(false);
        };
        let opened = view.stream.is_none();
        let stream = view
            .stream
            .get_or_insert_with(|| LiveStream::new(streaming.kind, &self.theme));
        let rendered = stream.advance(&streaming.text, self.width, &self.theme);
        let follows_own_kind = self
            .transcript
            .last()
            .and_then(streamed_part)
            .is_some_and(|(kind, _)| kind == streaming.kind);
        if opened && !follows_own_kind {
            self.write_output("\n")?;
        }
        if !rendered.is_empty() {
            self.write_stream_output(&rendered)?;
        }
        Ok(opened || !rendered.is_empty())
    }

    /// Shows the running tool's card: restarted when the tool changed or its output was rewritten,
    /// otherwise grown by the output streamed since the last sync. `output` is the latest output and
    /// `before` how many streamed bytes precede it; `tool_streamed` counts every byte shown so far.
    fn set_view_tool(&mut self, request: Option<ToolStartRequest>, output: &str, before: usize) -> io::Result<bool> {
        let shown = self.view.as_ref().map_or(0, |view| view.tool_streamed);
        let new_output = shown.checked_sub(before).and_then(|from| output.get(from..));
        let same = same_tool(self.state.active_tool(), request.as_ref());
        let delta = match new_output {
            Some(delta) if same => delta,
            _ => {
                match request {
                    Some(request) => self.start_tool(request)?,
                    None => self.end_tool()?,
                }
                output
            }
        };
        if let Some(view) = self.view.as_mut() {
            view.tool_streamed = before + output.len();
        }
        if !delta.is_empty() {
            self.append_tool_chunk(delta)?;
        }
        Ok(!same || new_output.is_none() || !delta.is_empty())
    }

    /// Keeps the main agent's output while another agent is shown.
    pub fn hold_output(&mut self, drained: PendingUiDrain) {
        if let Some(view) = self.view.as_mut() {
            super::held::hold(&mut view.parked.held, drained);
        }
    }

    /// Returns to the main agent: replays its transcript, then applies the output it produced
    /// while another agent was shown.
    pub fn leave_agent_view(&mut self) -> io::Result<()> {
        let Some(view) = self.view.take() else {
            return Ok(());
        };
        let parked = view.parked;
        self.state.footer_mut().viewing = None;
        self.transcript = parked.transcript;
        self.streamed_output = parked.streamed_output;
        self.cache.clear();
        self.state.footer_mut().running_tool = parked.active_tool.as_ref().map(|tool| tool.name.clone());
        self.state.set_active_tool(parked.active_tool);
        self.replay_transcript()?;
        for drained in parked.held {
            self.apply_output(drained)?;
        }
        Ok(())
    }

    /// Applies an agent's tool, output, and transcript updates; returns (state changed, output written).
    pub fn apply_output(&mut self, mut drained: PendingUiDrain) -> io::Result<(bool, bool)> {
        let mut changed = self.apply_tool_updates(&mut drained)?;
        let mut wrote_output = false;
        for output in drained.outputs {
            match output {
                crate::ui::interactive::OutputEvent::Text(text) => self.write_output(&text)?,
                crate::ui::interactive::OutputEvent::StreamText(text) => self.write_stream_output(&text)?,
            }
            wrote_output = true;
        }
        for item in drained.transcript_items {
            wrote_output |= self.push_transcript_item(item)?;
            changed = true;
        }
        if let Some(recap) = drained.trailing_recap {
            wrote_output |= self.upsert_trailing_recap(recap)?;
            changed = true;
        }
        Ok((changed, wrote_output))
    }

    fn apply_tool_updates(&mut self, drained: &mut PendingUiDrain) -> io::Result<bool> {
        let mut changed = false;
        if let Some(request) = drained.tool_start.take() {
            self.start_tool(request)?;
            changed = true;
        }
        if !drained.tool_chunks.is_empty() {
            self.append_tool_chunks(drained.tool_chunks.iter().map(String::as_str))?;
        }
        let has_tool_transcript = drained
            .transcript_items
            .iter()
            .any(|item| matches!(item, TranscriptItem::Tool(_)));
        if drained.tool_end && !has_tool_transcript {
            self.end_tool()?;
            changed = true;
        } else if !has_tool_transcript && let Some(running) = drained.running_tool.take() {
            self.state.footer_mut().running_tool = running;
            changed = true;
        }
        Ok(changed)
    }
}
