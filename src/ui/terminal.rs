use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, KeyCode, KeyEvent, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::{CompletedFrame, Frame, Terminal, TerminalOptions, Viewport};
use tokio::sync::mpsc;

use super::{ModalView, TerminalComponent, TerminalSurface};

pub const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
pub const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
pub const OSC133_ZONE_FINAL: &str = "\x1b]133;C\x07";
pub const TERMINAL_BELL: &str = "\x07";
pub const MOUSE_SCROLL_VELOCITY: u16 = 3;
pub const RESIZE_DEBOUNCE_MILLIS: u64 = 30;

static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn install_terminal_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show, DisableMouseCapture);
        default_hook(info);
    }));
}

pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, Show, EnableBracketedPaste)?;
        install_terminal_panic_hook();
        Ok(Self { active: true })
    }

    pub fn leave(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            let mut stdout = io::stdout();
            let _ = execute!(stdout, Show, DisableMouseCapture, DisableBracketedPaste);
            disable_raw_mode()?;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

pub fn write_turn_completion<W: Write>(
    writer: &mut W,
    prompt: &str,
    assistant_output: &str,
    is_focused: bool,
) -> io::Result<()> {
    writeln!(writer, "{OSC133_ZONE_START}")?;
    if !prompt.is_empty() {
        writeln!(writer, "{prompt}")?;
    }
    writeln!(writer, "{OSC133_ZONE_END}")?;
    if !assistant_output.is_empty() {
        writeln!(writer, "{assistant_output}")?;
    }
    write!(writer, "{OSC133_ZONE_FINAL}")?;
    if !is_focused {
        write!(writer, "{TERMINAL_BELL}")?;
    }
    writer.flush()
}

pub fn normalize_mouse_scroll(mouse: &MouseEvent) -> Option<(i32, u16)> {
    match mouse.kind {
        MouseEventKind::ScrollDown => Some((1, MOUSE_SCROLL_VELOCITY)),
        MouseEventKind::ScrollUp => Some((-1, MOUSE_SCROLL_VELOCITY)),
        _ => None,
    }
}

pub fn is_navigation_key(key: &KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
    )
}

#[derive(Debug, Default)]
pub struct KeyRepeatCoalescer {
    pending_key: Option<KeyEvent>,
    repeat_count: usize,
}

impl KeyRepeatCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, key: KeyEvent) -> Option<(KeyEvent, usize)> {
        if is_navigation_key(&key) {
            if let Some(prev) = self.pending_key {
                if prev.code == key.code && prev.modifiers == key.modifiers {
                    self.repeat_count += 1;
                    return None;
                }
                let flushed = (prev, self.repeat_count);
                self.pending_key = Some(key);
                self.repeat_count = 1;
                return Some(flushed);
            }
            self.pending_key = Some(key);
            self.repeat_count = 1;
            None
        } else {
            let flushed = self.flush();
            self.pending_key = None;
            self.repeat_count = 0;
            if flushed.is_some() { flushed } else { Some((key, 1)) }
        }
    }

    pub fn flush(&mut self) -> Option<(KeyEvent, usize)> {
        if let Some(key) = self.pending_key.take() {
            let count = self.repeat_count;
            self.repeat_count = 0;
            Some((key, count))
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct ResizeDebouncer {
    pending_dimensions: Option<(u16, u16)>,
    debounce_duration: Duration,
}

impl Default for ResizeDebouncer {
    fn default() -> Self {
        Self {
            pending_dimensions: None,
            debounce_duration: Duration::from_millis(RESIZE_DEBOUNCE_MILLIS),
        }
    }
}

impl ResizeDebouncer {
    pub fn new(debounce_millis: u64) -> Self {
        Self {
            pending_dimensions: None,
            debounce_duration: Duration::from_millis(debounce_millis),
        }
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.pending_dimensions = Some((width, height));
    }

    pub fn debounce_duration(&self) -> Duration {
        self.debounce_duration
    }

    pub fn take(&mut self) -> Option<(u16, u16)> {
        self.pending_dimensions.take()
    }

    pub fn pending(&self) -> Option<(u16, u16)> {
        self.pending_dimensions
    }
}

#[derive(Debug, Clone)]
pub struct NoticeSender {
    tx: mpsc::UnboundedSender<String>,
}

impl NoticeSender {
    pub fn send(&self, notice: impl Into<String>) {
        let _ = self.tx.send(notice.into());
    }
}

pub struct NoticeReceiver {
    rx: mpsc::UnboundedReceiver<String>,
}

impl NoticeReceiver {
    pub fn try_recv(&mut self) -> Option<String> {
        self.rx.try_recv().ok()
    }

    pub async fn recv(&mut self) -> Option<String> {
        self.rx.recv().await
    }
}

pub fn notice_channel() -> (NoticeSender, NoticeReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (NoticeSender { tx }, NoticeReceiver { rx })
}

fn crossterm_color(color: Color) -> crossterm::style::Color {
    match color {
        Color::Reset => crossterm::style::Color::Reset,
        Color::Black => crossterm::style::Color::Black,
        Color::Red => crossterm::style::Color::DarkRed,
        Color::Green => crossterm::style::Color::DarkGreen,
        Color::Yellow => crossterm::style::Color::DarkYellow,
        Color::Blue => crossterm::style::Color::DarkBlue,
        Color::Magenta => crossterm::style::Color::DarkMagenta,
        Color::Cyan => crossterm::style::Color::DarkCyan,
        Color::Gray => crossterm::style::Color::Grey,
        Color::DarkGray => crossterm::style::Color::DarkGrey,
        Color::LightRed => crossterm::style::Color::Red,
        Color::LightGreen => crossterm::style::Color::Green,
        Color::LightYellow => crossterm::style::Color::Yellow,
        Color::LightBlue => crossterm::style::Color::Blue,
        Color::LightMagenta => crossterm::style::Color::Magenta,
        Color::LightCyan => crossterm::style::Color::Cyan,
        Color::White => crossterm::style::Color::White,
        Color::Rgb(r, g, b) => crossterm::style::Color::Rgb { r, g, b },
        Color::Indexed(i) => crossterm::style::Color::AnsiValue(i),
    }
}

fn write_styled_cell<W: Write>(writer: &mut W, cell: &ratatui::buffer::Cell) -> io::Result<u16> {
    let style = cell.style();
    if let Some(fg) = style.fg {
        crossterm::execute!(writer, crossterm::style::SetForegroundColor(crossterm_color(fg)))?;
    }
    if let Some(bg) = style.bg {
        crossterm::execute!(writer, crossterm::style::SetBackgroundColor(crossterm_color(bg)))?;
    }
    if style.add_modifier.contains(Modifier::BOLD) {
        crossterm::execute!(
            writer,
            crossterm::style::SetAttribute(crossterm::style::Attribute::Bold)
        )?;
    }
    if style.add_modifier.contains(Modifier::DIM) {
        crossterm::execute!(writer, crossterm::style::SetAttribute(crossterm::style::Attribute::Dim))?;
    }
    if style.add_modifier.contains(Modifier::REVERSED) {
        crossterm::execute!(
            writer,
            crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)
        )?;
    }
    write!(writer, "{}", cell.symbol())?;
    crossterm::execute!(
        writer,
        crossterm::style::ResetColor,
        crossterm::style::SetAttribute(crossterm::style::Attribute::Reset)
    )?;
    Ok(unicode_width::UnicodeWidthStr::width(cell.symbol()) as u16)
}

pub struct SafeBackend<W: Write> {
    writer: W,
    cursor_pos: Position,
    height: u16,
}

impl<W: Write> SafeBackend<W> {
    pub fn new(mut writer: W, height: u16) -> io::Result<Self> {
        for _ in 0..height.saturating_sub(1) {
            writeln!(writer)?;
        }
        if height > 1 {
            write!(writer, "\x1b[{}A\r", height.saturating_sub(1))?;
        } else {
            write!(writer, "\r")?;
        }
        writer.flush()?;
        Ok(Self {
            writer,
            cursor_pos: Position::new(0, 0),
            height,
        })
    }

    pub fn prepare_frame(&mut self) -> io::Result<()> {
        if self.cursor_pos.y > 0 {
            write!(self.writer, "\x1b[{}A\r", self.cursor_pos.y)?;
            self.cursor_pos.y = 0;
            self.cursor_pos.x = 0;
        } else if self.cursor_pos.x > 0 {
            write!(self.writer, "\r")?;
            self.cursor_pos.x = 0;
        }
        Ok(())
    }

    pub fn print_scrollback(&mut self, text: &str) -> io::Result<()> {
        if self.cursor_pos.y > 0 {
            write!(self.writer, "\x1b[{}A\r", self.cursor_pos.y)?;
        } else {
            write!(self.writer, "\r")?;
        }
        write!(self.writer, "\x1b[J")?;
        write!(self.writer, "{text}")?;
        if !text.ends_with('\n') {
            writeln!(self.writer)?;
        }
        for _ in 0..self.height.saturating_sub(1) {
            writeln!(self.writer)?;
        }
        if self.height > 1 {
            write!(self.writer, "\x1b[{}A\r", self.height.saturating_sub(1))?;
        } else {
            write!(self.writer, "\r")?;
        }
        self.cursor_pos = Position::new(0, 0);
        self.writer.flush()
    }
}

impl<W: Write> Backend for SafeBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        for (x, y, cell) in content {
            self.set_cursor_position(Position::new(x, y))?;
            let w = write_styled_cell(&mut self.writer, cell)?;
            self.cursor_pos.x += w;
        }
        self.writer.flush()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        crossterm::execute!(self.writer, crossterm::cursor::Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        crossterm::execute!(self.writer, crossterm::cursor::Show)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor_pos)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let target = position.into();
        let dy = target.y as i32 - self.cursor_pos.y as i32;
        if dy > 0 {
            write!(self.writer, "\x1b[{}B", dy)?;
        } else if dy < 0 {
            write!(self.writer, "\x1b[{}A", -dy)?;
        }
        write!(self.writer, "\x1b[{}G", target.x + 1)?;
        self.cursor_pos = target;
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        if self.cursor_pos.y > 0 {
            write!(self.writer, "\x1b[{}A\r", self.cursor_pos.y)?;
            self.cursor_pos.y = 0;
            self.cursor_pos.x = 0;
        } else {
            write!(self.writer, "\r")?;
            self.cursor_pos.x = 0;
        }
        write!(self.writer, "\x1b[J")?;
        self.writer.flush()
    }

    fn clear_region(&mut self, _clear_type: ratatui::backend::ClearType) -> io::Result<()> {
        self.clear()
    }

    fn size(&self) -> io::Result<ratatui::layout::Size> {
        let (w, _) = crossterm::terminal::size().unwrap_or((80, 24));
        Ok(ratatui::layout::Size::new(w, self.height))
    }

    fn window_size(&mut self) -> io::Result<ratatui::backend::WindowSize> {
        let (w, _) = crossterm::terminal::size().unwrap_or((80, 24));
        Ok(ratatui::backend::WindowSize {
            columns_rows: ratatui::layout::Size::new(w, self.height),
            pixels: ratatui::layout::Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub struct TerminalRunner<B: Backend> {
    terminal: Terminal<B>,
    viewport_height: u16,
    focused: bool,
    active: bool,
    guard: Option<TerminalGuard>,
}

impl TerminalRunner<SafeBackend<io::Stdout>> {
    pub fn from_stdout(viewport_height: u16) -> io::Result<Self> {
        let guard = TerminalGuard::enter()?;
        let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
        let backend = SafeBackend::new(io::stdout(), viewport_height)?;
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, width, viewport_height)),
        };
        let terminal = Terminal::with_options(backend, options)?;
        Ok(Self {
            terminal,
            viewport_height,
            focused: true,
            active: true,
            guard: Some(guard),
        })
    }

    pub fn draw<F>(&mut self, f: F) -> io::Result<CompletedFrame<'_>>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.backend_mut().prepare_frame()?;
        self.terminal.draw(f).map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn print_scrollback(&mut self, text: &str) -> io::Result<()> {
        self.terminal.backend_mut().print_scrollback(text)
    }
}

impl TerminalRunner<TestBackend> {
    pub fn headless(width: u16, height: u16) -> Self {
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend).expect("valid test backend");
        Self {
            terminal,
            viewport_height: height,
            focused: true,
            active: true,
            guard: None,
        }
    }

    pub fn backend(&self) -> &TestBackend {
        self.terminal.backend()
    }

    pub fn backend_mut(&mut self) -> &mut TestBackend {
        self.terminal.backend_mut()
    }
}

impl<B: Backend> TerminalRunner<B> {
    pub fn new(backend: B, viewport_height: u16) -> io::Result<Self> {
        let options = TerminalOptions {
            viewport: Viewport::Inline(viewport_height),
        };
        let terminal = Terminal::with_options(backend, options).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self {
            terminal,
            viewport_height,
            focused: true,
            active: true,
            guard: None,
        })
    }

    pub fn viewport_height(&self) -> u16 {
        self.viewport_height
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        if let Some(guard) = &mut self.guard {
            guard.leave()?;
        }
        self.active = false;
        Ok(())
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if self.active {
            return Ok(());
        }
        if self.guard.is_some() {
            self.guard = Some(TerminalGuard::enter()?);
        }
        self.active = true;
        self.terminal.clear().map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear().map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn autoresize(&mut self) -> io::Result<()> {
        self.terminal.autoresize().map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn commit_turn(&mut self, prompt: &str, assistant_output: &str) -> io::Result<()> {
        self.terminal.clear().map_err(|e| io::Error::other(e.to_string()))?;
        let mut stdout = io::stdout();
        write_turn_completion(&mut stdout, prompt, assistant_output, self.focused)?;
        Ok(())
    }

    pub fn render_component(
        &mut self,
        component: &dyn TerminalComponent,
        area: Rect,
    ) -> io::Result<CompletedFrame<'_>> {
        self.terminal
            .draw(|frame| {
                component.render(frame, area);
            })
            .map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn render_modal(&mut self, modal: &dyn ModalView, area: Rect) -> io::Result<CompletedFrame<'_>> {
        self.terminal
            .draw(|frame| {
                modal.render(frame, area);
            })
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

impl<B: Backend> TerminalSurface for TerminalRunner<B> {
    fn size(&self) -> io::Result<Rect> {
        let size = self.terminal.size().map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Rect::new(0, 0, size.width, size.height))
    }

    fn draw<F>(&mut self, f: F) -> io::Result<CompletedFrame<'_>>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.draw(f).map_err(|e| io::Error::other(e.to_string()))
    }

    fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear().map_err(|e| io::Error::other(e.to_string()))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.terminal.hide_cursor().map_err(|e| io::Error::other(e.to_string()))
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.terminal.show_cursor().map_err(|e| io::Error::other(e.to_string()))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.terminal
            .set_cursor_position(position)
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

#[cfg(unix)]
pub async fn handle_job_suspension<B: Backend>(runner: &mut TerminalRunner<B>) -> io::Result<()> {
    runner.suspend()?;
    unsafe {
        libc::signal(libc::SIGTSTP, libc::SIG_DFL);
        libc::raise(libc::SIGTSTP);
    }
    runner.resume()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::Paragraph;

    #[test]
    fn test_turn_completion_writer_formats_osc133_and_bell() {
        let mut output = Vec::new();
        write_turn_completion(&mut output, "hello world", "hi there!", false).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.starts_with(OSC133_ZONE_START));
        assert!(rendered.contains("hello world"));
        assert!(rendered.contains(OSC133_ZONE_END));
        assert!(rendered.contains("hi there!"));
        assert!(rendered.ends_with(&format!("{OSC133_ZONE_FINAL}{TERMINAL_BELL}")));
    }

    #[test]
    fn test_turn_completion_writer_focused_omits_bell() {
        let mut output = Vec::new();
        write_turn_completion(&mut output, "hello world", "hi there!", true).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(!rendered.ends_with(&format!("{OSC133_ZONE_FINAL}{TERMINAL_BELL}")));
        assert!(rendered.ends_with(OSC133_ZONE_FINAL));
    }

    #[test]
    fn test_mouse_scroll_normalization() {
        let down_event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let up_event = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let move_event = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 10,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        assert_eq!(normalize_mouse_scroll(&down_event), Some((1, 3)));
        assert_eq!(normalize_mouse_scroll(&up_event), Some((-1, 3)));
        assert_eq!(normalize_mouse_scroll(&move_event), None);
    }

    #[test]
    fn test_key_repeat_coalescer_accumulates_nav_keys() {
        let mut coalescer = KeyRepeatCoalescer::new();
        let down = KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::empty());
        assert_eq!(coalescer.push(down), None);
        assert_eq!(coalescer.push(down), None);
        assert_eq!(coalescer.push(down), None);

        let up = KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::empty());
        let flushed = coalescer.push(up);
        assert_eq!(flushed, Some((down, 3)));

        let final_flush = coalescer.flush();
        assert_eq!(final_flush, Some((up, 1)));
    }

    #[test]
    fn test_resize_debouncer_coalesces_bursts() {
        let mut debouncer = ResizeDebouncer::new(30);
        debouncer.on_resize(80, 24);
        debouncer.on_resize(85, 25);
        debouncer.on_resize(90, 26);
        assert_eq!(debouncer.pending(), Some((90, 26)));
        assert_eq!(debouncer.take(), Some((90, 26)));
        assert_eq!(debouncer.take(), None);
    }

    #[test]
    fn test_headless_terminal_runner_drawing() {
        let mut runner = TerminalRunner::headless(80, 24);
        runner
            .draw(|frame| {
                let area = frame.area();
                let paragraph = Paragraph::new("Headless Ratatui Engine Active");
                frame.render_widget(paragraph, area);
            })
            .unwrap();

        let buffer = runner.backend().buffer();
        let content = format!("{buffer:?}");
        assert!(content.contains("Headless Ratatui Engine Active"));
    }

    #[test]
    fn test_notice_channel_routes_messages() {
        let (sender, mut receiver) = notice_channel();
        sender.send("background compaction complete");
        let msg = receiver.try_recv();
        assert_eq!(msg.as_deref(), Some("background compaction complete"));
    }

    #[test]
    fn test_suspend_and_resume_lifecycle() {
        let mut runner = TerminalRunner::headless(80, 10);
        assert!(runner.is_active());
        runner.suspend().unwrap();
        assert!(!runner.is_active());
        runner.resume().unwrap();
        assert!(runner.is_active());
    }

    struct MockComponent;
    impl TerminalComponent for MockComponent {
        fn render(&self, frame: &mut Frame, area: Rect) {
            frame.render_widget(Paragraph::new("mock component rendered"), area);
        }
    }

    #[test]
    fn test_render_component_trait() {
        let mut runner = TerminalRunner::headless(60, 10);
        let comp = MockComponent;
        runner.render_component(&comp, Rect::new(0, 0, 60, 5)).unwrap();
        let content = format!("{:?}", runner.backend().buffer());
        assert!(content.contains("mock component rendered"));
    }
}
