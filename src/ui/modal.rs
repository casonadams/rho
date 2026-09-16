use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};

use rho_harness_core::presentation::InteractionInput;
use rho_harness_core::session::SessionSummary;
use rho_ui_core::autocomplete::{AutocompleteCandidate, THINKING_LEVEL_OPTIONS};
pub use rho_ui_core::format_relative_time;
use rho_ui_core::modal::{McpModalState, ModalOption, ModalState, ModelRegistry, SettingsState, SkillModalState};
use rho_ui_core::session::ProviderDef;
use rho_ui_core::state::RhoTicket;
use std::collections::HashMap;

use super::ModalView;

pub const MODAL_BG: Color = Color::Rgb(18, 20, 24);

pub fn centered_modal_area(width_req: u16, height_req: u16, area: Rect) -> Rect {
    let width = width_req.min(area.width.saturating_sub(2)).max(20);
    let height = height_req.min(area.height.saturating_sub(2)).max(5);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn build_modal_items<'a>(state: &'a ModalState, list_width: usize) -> Vec<ListItem<'a>> {
    state
        .filtered_options
        .iter()
        .enumerate()
        .map(|(idx, opt)| {
            let is_selected = idx == state.selected_index;
            let mut spans = Vec::new();

            if state.filter_query.is_empty() && (1..=9).contains(&(idx + 1)) {
                spans.push(Span::styled(
                    format!("{}. ", idx + 1),
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                spans.push(Span::raw("   "));
            }

            let label_style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(format!("{:<16}", opt.label), label_style));

            let reserved_w = 3 + 16 + if opt.is_active { 4 } else { 0 };
            let avail_desc_w = list_width.saturating_sub(reserved_w);

            if let Some(desc) = &opt.description {
                let clean_desc: String = desc.chars().take(avail_desc_w).collect();
                spans.push(Span::styled(
                    format!("  {clean_desc}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            if opt.is_active {
                spans.push(Span::styled(
                    "  ✓",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ));
            }

            let item_style = if is_selected {
                Style::default().bg(Color::Rgb(30, 45, 65))
            } else {
                Style::default().bg(MODAL_BG)
            };

            ListItem::new(Line::from(spans)).style(item_style)
        })
        .collect()
}

/// Inline text entry hosted at the bottom of a modal, driven by an option that
/// carries an `InteractionInput` spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineInput {
    pub label: String,
    pub option_value: String,
    pub text: String,
    pub cursor: usize,
    pub submitted: bool,
}

impl InlineInput {
    fn new(label: String, option_value: String, value: Option<String>) -> Self {
        let text = value.unwrap_or_default();
        let cursor = text.chars().count();
        Self {
            label,
            option_value,
            text,
            cursor,
            submitted: false,
        }
    }

    fn byte_offset(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let at = self.byte_offset(self.cursor - 1);
        self.text.remove(at);
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let at = self.byte_offset(self.cursor);
        self.text.remove(at);
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.text.clear();
                self.cursor = 0;
            }
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, _) => self.delete(),
            (KeyCode::Left, _) => self.cursor = self.cursor.saturating_sub(1),
            (KeyCode::Right, _) => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            (KeyCode::Home, _) => self.cursor = 0,
            (KeyCode::End, _) => self.cursor = self.text.chars().count(),
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => self.insert(c),
            _ => {}
        }
    }
}

fn render_inline_input(frame: &mut Frame, inline: &InlineInput, area: Rect) {
    let before: String = inline.text.chars().take(inline.cursor).collect();
    let after: String = inline.text.chars().skip(inline.cursor).collect();
    let entry = Line::from(vec![
        Span::styled(
            format!("{} > ", inline.label),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(before),
        Span::styled("\u{258f}", Style::default().fg(Color::Cyan)),
        Span::raw(after),
    ]);
    let hint = Line::from(Span::styled(
        "Enter submit \u{2022} Esc back",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(
        Paragraph::new(vec![entry, hint]).style(Style::default().bg(MODAL_BG)),
        area,
    );
}

pub fn render_modal(frame: &mut Frame, state: &ModalState, area: Rect, list_state: &mut ListState) {
    render_modal_inner(frame, state, area, list_state, None);
}

fn render_subtitle_area(frame: &mut Frame, subtitle: &str, content_area: Rect) -> (Option<Rect>, Rect) {
    if subtitle.is_empty() {
        return (None, content_area);
    }
    let lines: Vec<&str> = subtitle.lines().collect();
    let subtitle_height = (lines.len() as u16 + 1).min(content_area.height.saturating_sub(3));
    if subtitle_height == 0 {
        return (None, content_area);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(subtitle_height), Constraint::Min(1)])
        .split(content_area);
    let rendered_lines: Vec<Line> = subtitle
        .lines()
        .map(|l| {
            if let Some((k, v)) = l.split_once(": ") {
                Line::from(vec![
                    Span::styled(format!("{k}: "), Style::default().fg(Color::DarkGray)),
                    Span::styled(v.to_string(), Style::default().fg(Color::White)),
                ])
            } else {
                Line::from(Span::styled(l.to_string(), Style::default().fg(Color::White)))
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(rendered_lines).style(Style::default().bg(MODAL_BG)),
        chunks[0],
    );
    (Some(chunks[0]), chunks[1])
}

fn render_modal_inner(
    frame: &mut Frame,
    state: &ModalState,
    area: Rect,
    list_state: &mut ListState,
    inline: Option<&InlineInput>,
) {
    let modal_w = if area.width <= 70 {
        area.width.saturating_sub(2).max(20)
    } else {
        (area.width * 7 / 10).clamp(50, 90)
    };
    let modal_h = (area.height * 7 / 10).clamp(8, 25);
    let popup_area = centered_modal_area(modal_w, modal_h, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(MODAL_BG))
        .title(Span::styled(
            format!(" {} ", state.title),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
    let inner_area = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner_area.height == 0 || inner_area.width == 0 {
        return;
    }

    let (content_area, input_area) = match inline {
        Some(_) if inner_area.height >= 3 => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(2)])
                .split(inner_area);
            (chunks[0], Some(chunks[1]))
        }
        _ => (inner_area, None),
    };

    if let (Some(inline), Some(input_area)) = (inline, input_area) {
        render_inline_input(frame, inline, input_area);
    }

    let (_body_area, list_container) = render_subtitle_area(frame, &state.subtitle, content_area);

    let (search_area, list_area) = if state.search_enabled {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(list_container);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, list_container)
    };

    if let Some(sa) = search_area {
        let query_text = if state.filter_query.is_empty() {
            Span::styled("> Type to search...", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(format!("> {}", state.filter_query), Style::default().fg(Color::Yellow))
        };
        frame.render_widget(Paragraph::new(query_text).style(Style::default().bg(MODAL_BG)), sa);
    }

    let items = build_modal_items(state, list_area.width as usize);
    list_state.select(Some(state.selected_index));
    let list = List::new(items).style(Style::default().bg(MODAL_BG));
    frame.render_stateful_widget(list, list_area, list_state);
}

#[derive(Debug, Clone)]
pub struct StandardModalView {
    pub state: ModalState,
    pub list_state: ListState,
    inline_inputs: HashMap<String, InteractionInput>,
    active_input: Option<InlineInput>,
}

impl StandardModalView {
    pub fn new(state: ModalState) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(state.selected_index));
        Self {
            state,
            list_state,
            inline_inputs: HashMap::new(),
            active_input: None,
        }
    }

    /// Registers per-option inline inputs keyed by `ModalOption::value`, so an
    /// option carrying an input spec collects text before the modal resolves.
    pub fn with_inline_inputs(mut self, inputs: impl IntoIterator<Item = (String, InteractionInput)>) -> Self {
        self.inline_inputs = inputs.into_iter().collect();
        self
    }

    pub fn active_input(&self) -> Option<&InlineInput> {
        self.active_input.as_ref()
    }

    /// Returns the option value and submitted text once an inline input is confirmed.
    pub fn submitted_input(&self) -> Option<(&str, &str)> {
        self.active_input
            .as_ref()
            .filter(|input| input.submitted)
            .map(|input| (input.option_value.as_str(), input.text.as_str()))
    }

    fn begin_inline_input(&mut self) -> bool {
        let Some(option) = self.state.selected_option() else {
            return false;
        };
        let Some(spec) = self.inline_inputs.get(&option.value) else {
            return false;
        };
        self.active_input = Some(InlineInput::new(
            spec.label.clone(),
            option.value.clone(),
            spec.value.clone(),
        ));
        true
    }

    pub fn thinking(active_level: Option<&str>) -> Self {
        let active = active_level.unwrap_or("off");
        let options = THINKING_LEVEL_OPTIONS
            .iter()
            .map(|(level, desc)| {
                let is_active = *level == active;
                ModalOption {
                    label: format!("{level:<12}"),
                    description: Some(desc.to_string()),
                    value: level.to_string(),
                    is_active,
                    shortcut: None,
                }
            })
            .collect();
        let modal = ModalState::new("Select Thinking Level", options).with_search(false);
        Self::new(modal)
    }

    pub fn model(registry: &ModelRegistry) -> Self {
        Self::new(registry.to_modal_state())
    }

    pub fn auth(providers: &[ProviderDef], active_provider: Option<&str>) -> Self {
        let options = providers
            .iter()
            .map(|p| {
                let is_active = active_provider.is_some_and(|ap| ap == p.id);
                let active_badge = if is_active { "  ✓" } else { "" };
                ModalOption {
                    label: format!("{:<16}", p.id),
                    description: Some(format!(
                        "{} [{:?}] · {}{}",
                        p.name, p.auth_mode, p.description, active_badge
                    )),
                    value: p.id.to_string(),
                    is_active,
                    shortcut: None,
                }
            })
            .collect();
        let modal = ModalState::new("Login Provider", options);
        Self::new(modal)
    }

    pub fn mcp(mcp_state: &McpModalState) -> Self {
        Self::new(mcp_state.modal.clone())
    }

    pub fn skill(skill_state: &SkillModalState) -> Self {
        Self::new(skill_state.modal.clone())
    }

    pub fn session(summaries: &[SessionSummary], active_id: Option<&str>) -> Self {
        let options = summaries
            .iter()
            .map(|s| {
                let is_active = active_id.is_some_and(|aid| aid == s.session_id);
                let active_badge = if is_active { "  ✓" } else { "" };
                let display_title = s.name.as_deref().unwrap_or(&s.session_id);
                let rel_time = format_relative_time(s.last_modified);
                ModalOption {
                    label: format!("{:<18}", display_title),
                    description: Some(format!("{} turns · {}{}", s.turn_count, rel_time, active_badge)),
                    value: s.session_id.clone(),
                    is_active,
                    shortcut: None,
                }
            })
            .collect();
        let modal = ModalState::new("Resume Session", options);
        Self::new(modal)
    }

    pub fn settings(settings: &SettingsState) -> Self {
        Self::new(settings.to_modal_state().with_search(false))
    }
}

pub fn run_modal_view<V: ModalView>(view: &mut V) -> std::io::Result<bool> {
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            view.render(f, area);
        })?;

        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            if key.kind == crossterm::event::KeyEventKind::Release {
                continue;
            }
            if key.code == KeyCode::Enter {
                view.handle_key(key);
                if !view.is_open() {
                    return Ok(false);
                }
                if view.is_submitted() {
                    return Ok(true);
                }
                continue;
            }
            view.handle_key(key);
            if !view.is_open() {
                return Ok(false);
            }
        }
    }
}

pub async fn run_modal_view_async<V: ModalView>(
    view: &mut V,
    events: &mut crossterm::event::EventStream,
) -> std::io::Result<bool> {
    use futures::StreamExt;
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            view.render(f, area);
        })?;

        let Some(Ok(event)) = events.next().await else {
            return Ok(false);
        };

        if let crossterm::event::Event::Key(key) = event {
            if key.kind == crossterm::event::KeyEventKind::Release {
                continue;
            }
            if key.code == KeyCode::Enter {
                view.handle_key(key);
                if !view.is_open() {
                    return Ok(false);
                }
                if view.is_submitted() {
                    return Ok(true);
                }
                continue;
            }
            view.handle_key(key);
            if !view.is_open() {
                return Ok(false);
            }
        }
    }
}

pub fn prompt_session_picker(sessions_dir: &std::path::Path) -> rho_harness_core::error::Result<Option<String>> {
    let summaries = rho_harness_core::session::SessionManager::list_session_summaries(sessions_dir)?;
    if summaries.is_empty() {
        return Ok(None);
    }
    let mut view = StandardModalView::session(&summaries, None);
    let guard = crate::ui::terminal::TerminalGuard::enter()?;
    let selected = if run_modal_view(&mut view)? {
        view.state.selected_option().map(|opt| opt.value.clone())
    } else {
        None
    };
    drop(guard);
    Ok(selected)
}

impl ModalView for StandardModalView {
    fn title(&self) -> &str {
        &self.state.title
    }

    fn selected_index(&self) -> usize {
        self.state.selected_index
    }

    fn item_count(&self) -> usize {
        self.state.filtered_options.len()
    }

    fn filter(&self) -> &str {
        &self.state.filter_query
    }

    fn set_filter(&mut self, query: &str) {
        self.state.set_filter(query);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(input) = self.active_input.as_mut() {
            match key.code {
                KeyCode::Enter => input.submitted = true,
                KeyCode::Esc => self.active_input = None,
                _ => input.handle_key(key),
            }
            return true;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                self.begin_inline_input();
                true
            }
            (KeyCode::Esc, _) => {
                self.state.close();
                true
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if !self.state.filter_query.is_empty() {
                    self.state.set_filter("");
                } else {
                    self.state.close();
                }
                true
            }
            (KeyCode::Up, _) => {
                self.state.select_prev();
                true
            }
            (KeyCode::Char('k'), KeyModifiers::NONE)
                if !self.state.search_enabled || self.state.filter_query.is_empty() =>
            {
                self.state.select_prev();
                true
            }
            (KeyCode::Down, _) => {
                self.state.select_next();
                true
            }
            (KeyCode::Char('j'), KeyModifiers::NONE)
                if !self.state.search_enabled || self.state.filter_query.is_empty() =>
            {
                self.state.select_next();
                true
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.state.select_next();
                true
            }
            (KeyCode::BackTab, _) | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.state.select_prev();
                true
            }
            (KeyCode::Char(c), KeyModifiers::NONE)
                if ('1'..='9').contains(&c) && self.state.filter_query.is_empty() =>
            {
                let digit = (c as u8) - b'0';
                self.state.select_digit(digit);
                true
            }
            (KeyCode::Backspace, _) if self.state.search_enabled => {
                self.state.pop_filter_char();
                true
            }
            (KeyCode::Char(c), KeyModifiers::NONE) if self.state.search_enabled => {
                self.state.append_filter_char(c);
                true
            }
            _ => false,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let mut list_state = self.list_state;
        render_modal_inner(frame, &self.state, area, &mut list_state, self.active_input.as_ref());
    }

    fn is_open(&self) -> bool {
        self.state.is_open
    }

    fn is_submitted(&self) -> bool {
        self.active_input.as_ref().is_none_or(|input| input.submitted)
    }
}

pub fn render_modal_lines(view: &StandardModalView, width: usize) -> Vec<String> {
    let width = (width.max(30) as u16).min(120);
    let height = (view.state.filtered_options.len() as u16 + 6).clamp(8, 16);
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("valid test backend");
    let _ = terminal.draw(|f| {
        view.render(f, Rect::new(0, 0, width, height));
    });
    let buf = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        let mut has_content = false;
        for x in 0..width {
            let sym = buf[(x, y)].symbol();
            line.push_str(sym);
            if !sym.trim().is_empty() {
                has_content = true;
            }
        }
        if has_content {
            lines.push(line.trim_end().to_string());
        }
    }
    lines
}

#[derive(Debug, Clone)]
pub struct RemotePairModalView {
    pub ticket: RhoTicket,
    pub is_open: bool,
}

impl RemotePairModalView {
    pub fn new(ticket: RhoTicket) -> Self {
        Self { ticket, is_open: true }
    }

    pub fn render_qr_code(&self) -> Option<String> {
        let ticket_str = self.ticket.to_string_repr();
        if let Ok(code) = qrcode::QrCode::new(ticket_str.as_bytes()) {
            Some(code.render::<char>().quiet_zone(false).module_dimensions(2, 1).build())
        } else {
            None
        }
    }
}

impl ModalView for RemotePairModalView {
    fn title(&self) -> &str {
        "Pair Remote Node"
    }

    fn selected_index(&self) -> usize {
        0
    }

    fn item_count(&self) -> usize {
        1
    }

    fn filter(&self) -> &str {
        ""
    }

    fn set_filter(&mut self, _query: &str) {}

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.is_open = false;
                true
            }
            _ => false,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let popup_w = (area.width * 8 / 10).clamp(50, 95);
        let popup_h = (area.height * 8 / 10).clamp(12, 30);
        let popup_area = centered_modal_area(popup_w, popup_h, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Magenta))
            .style(Style::default().bg(MODAL_BG))
            .title(Span::styled(
                " Pair Remote Node ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 4 || inner.width < 10 {
            return;
        }

        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            "Scan QR code with Web Hub or use ticket below:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            self.ticket.to_string_repr(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if let Some(qr_art) = self.render_qr_code() {
            for qr_line in qr_art.lines() {
                lines.push(Line::from(Span::styled(
                    qr_line.to_string(),
                    Style::default().fg(Color::White),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[Esc] Close",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    fn is_open(&self) -> bool {
        self.is_open
    }
}

#[derive(Debug, Clone)]
pub struct AutocompletePopupView {
    pub candidates: Vec<AutocompleteCandidate>,
    pub selected_index: usize,
    pub is_open: bool,
}

impl AutocompletePopupView {
    pub fn new(candidates: Vec<AutocompleteCandidate>) -> Self {
        Self {
            candidates,
            selected_index: 0,
            is_open: true,
        }
    }

    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.candidates.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.candidates.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    pub fn selected(&self) -> Option<&AutocompleteCandidate> {
        self.candidates.get(self.selected_index)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.is_open = false;
                true
            }
            (KeyCode::Up, _) => {
                self.select_prev();
                true
            }
            (KeyCode::Down, _) => {
                self.select_next();
                true
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.select_next();
                true
            }
            (KeyCode::BackTab, _) | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.select_prev();
                true
            }
            _ => false,
        }
    }

    pub fn render_anchored(&self, frame: &mut Frame, cursor_area: Rect, container_area: Rect) {
        if self.candidates.is_empty() || !self.is_open {
            return;
        }

        let max_label_w = self
            .candidates
            .iter()
            .map(|c| c.display.len() + c.description.as_ref().map_or(0, |d| d.len() + 3))
            .max()
            .unwrap_or(20);
        let popup_w = (max_label_w as u16 + 4).clamp(30, container_area.width.saturating_sub(4));
        let popup_h = (self.candidates.len() as u16 + 2).min(8);

        let popup_y = if cursor_area.y >= popup_h + container_area.y {
            cursor_area.y - popup_h
        } else {
            cursor_area.y + cursor_area.height
        };
        let popup_x = cursor_area
            .x
            .min(container_area.width.saturating_sub(popup_w) + container_area.x);
        let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(MODAL_BG))
            .title(Span::styled(
                " Autocomplete ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let items: Vec<ListItem> = self
            .candidates
            .iter()
            .enumerate()
            .map(|(idx, cand)| {
                let is_selected = idx == self.selected_index;
                let label_style = if is_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let mut spans = vec![Span::styled(cand.display.clone(), label_style)];
                if let Some(desc) = &cand.description {
                    spans.push(Span::styled(format!("  {desc}"), Style::default().fg(Color::DarkGray)));
                }

                let item_style = if is_selected {
                    Style::default().bg(Color::Rgb(30, 45, 65))
                } else {
                    Style::default().bg(MODAL_BG)
                };

                ListItem::new(Line::from(spans)).style(item_style)
            })
            .collect();

        let list = List::new(items).style(Style::default().bg(MODAL_BG));
        frame.render_widget(list, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rho_ui_core::modal::ModelCapability;
    use rho_ui_core::session::AuthMode;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn buffer_text(backend: &TestBackend) -> String {
        let mut s = String::new();
        let buf = backend.buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn inline_input_view() -> StandardModalView {
        let options = vec![
            ModalOption {
                label: "allow".to_string(),
                description: None,
                value: "0".to_string(),
                is_active: false,
                shortcut: None,
            },
            ModalOption {
                label: "deny".to_string(),
                description: None,
                value: "1".to_string(),
                is_active: false,
                shortcut: None,
            },
        ];
        let state = ModalState::new("Approve Tool", options).with_search(false);
        StandardModalView::new(state).with_inline_inputs([(
            "1".to_string(),
            InteractionInput {
                label: "reason".to_string(),
                value: None,
            },
        )])
    }

    #[test]
    fn option_with_input_spec_collects_text_before_submitting() {
        let mut view = inline_input_view();
        view.state.select_next();

        view.handle_key(key(KeyCode::Enter));
        assert!(view.active_input().is_some(), "Enter must open the inline input");
        assert!(!view.is_submitted(), "modal must stay open to collect text");
        assert!(view.is_open());

        for c in "nope".chars() {
            view.handle_key(key(KeyCode::Char(c)));
        }
        view.handle_key(key(KeyCode::Enter));

        assert!(view.is_submitted());
        assert_eq!(view.submitted_input(), Some(("1", "nope")));
    }

    #[test]
    fn option_without_input_spec_submits_on_first_enter() {
        let mut view = inline_input_view();

        view.handle_key(key(KeyCode::Enter));

        assert!(view.active_input().is_none());
        assert!(view.is_submitted());
        assert_eq!(view.submitted_input(), None);
    }

    #[test]
    fn escape_leaves_inline_input_without_closing_modal() {
        let mut view = inline_input_view();
        view.state.select_next();
        view.handle_key(key(KeyCode::Enter));
        view.handle_key(key(KeyCode::Char('x')));

        view.handle_key(key(KeyCode::Esc));

        assert!(view.active_input().is_none(), "Esc returns to option selection");
        assert!(view.is_open(), "Esc from input must not dismiss the modal");
        assert_eq!(view.submitted_input(), None);
    }

    #[test]
    fn inline_input_edits_text_at_cursor() {
        let mut view = inline_input_view();
        view.state.select_next();
        view.handle_key(key(KeyCode::Enter));

        for c in "abc".chars() {
            view.handle_key(key(KeyCode::Char(c)));
        }
        view.handle_key(key(KeyCode::Left));
        view.handle_key(key(KeyCode::Char('Z')));
        view.handle_key(key(KeyCode::Backspace));
        view.handle_key(key(KeyCode::Home));
        view.handle_key(key(KeyCode::Delete));

        let input = view.active_input().expect("input still active");
        assert_eq!(input.text, "bc");
    }

    #[test]
    fn inline_input_renders_label_and_hint() {
        let mut view = inline_input_view();
        view.state.select_next();
        view.handle_key(key(KeyCode::Enter));
        view.handle_key(key(KeyCode::Char('h')));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| view.render(f, f.area())).unwrap();
        let rendered = buffer_text(terminal.backend());

        assert!(rendered.contains("reason >"), "input label must render");
        assert!(rendered.contains('h'));
        assert!(rendered.contains("Enter submit"));
    }

    #[test]
    fn modal_renders_subtitle_body_above_options() {
        let options = vec![ModalOption {
            label: "Allow".to_string(),
            description: Some("Run this tool call once".to_string()),
            value: "0".to_string(),
            is_active: false,
            shortcut: None,
        }];
        let mut state = ModalState::new("Permission Required", options).with_search(false);
        state.subtitle = "Tool: bash\nInput: rm -rf /tmp/scratch".to_string();
        let view = StandardModalView::new(state);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| view.render(f, f.area())).unwrap();
        let rendered = buffer_text(terminal.backend());

        assert!(rendered.contains("Tool: bash"), "subtitle tool line must render");
        assert!(
            rendered.contains("rm -rf /tmp/scratch"),
            "approval target must be visible before selecting an option"
        );
        assert!(
            rendered.contains("Allow"),
            "options must still render below the subtitle"
        );
    }

    #[test]
    fn test_render_modal_across_terminal_dimensions() {
        let dimensions = [(80, 24), (120, 40), (60, 15)];
        let view = StandardModalView::thinking(Some("medium"));

        for (w, h) in dimensions {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| {
                    view.render(f, Rect::new(0, 0, w, h));
                })
                .unwrap();
            let text = buffer_text(terminal.backend());
            assert!(text.contains("Select Thinking Level"), "dim ({w}, {h}) missing title");
            assert!(text.contains("medium"), "dim ({w}, {h}) missing option");
            assert!(text.contains('✓'), "dim ({w}, {h}) missing active checkmark");
        }
    }

    #[test]
    fn test_standard_modal_thinking_navigation_and_jump_keys() {
        let mut view = StandardModalView::thinking(Some("low"));
        assert_eq!(view.title(), "Select Thinking Level");
        assert_eq!(view.selected_index(), 0);

        assert!(view.handle_key(key(KeyCode::Char('3'))));
        assert_eq!(view.selected_index(), 2);
        assert_eq!(view.state.selected_option().unwrap().value, "medium");

        assert!(view.handle_key(key(KeyCode::Down)));
        assert_eq!(view.selected_index(), 3);
        assert!(view.handle_key(key(KeyCode::Up)));
        assert_eq!(view.selected_index(), 2);

        assert!(view.handle_key(key(KeyCode::Tab)));
        assert_eq!(view.selected_index(), 3);
        assert!(view.handle_key(key(KeyCode::BackTab)));
        assert_eq!(view.selected_index(), 2);
    }

    fn test_models() -> Vec<ModelCapability> {
        vec![
            ModelCapability {
                id: "claude-sonnet-3-5".to_string(),
                provider: "anthropic".to_string(),
                context_tokens: 200_000,
                supports_reasoning: true,
                is_local: false,
                display_name: "Claude 3.5 Sonnet".to_string(),
            },
            ModelCapability {
                id: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                context_tokens: 128_000,
                supports_reasoning: false,
                is_local: false,
                display_name: "GPT-4o".to_string(),
            },
        ]
    }

    #[test]
    fn test_standard_modal_search_filtering() {
        let registry = ModelRegistry::new(test_models(), "claude-sonnet-3-5".to_string());
        let mut view = StandardModalView::model(&registry);
        assert_eq!(view.item_count(), 2);

        assert!(view.handle_key(key(KeyCode::Char('g'))));
        assert!(view.handle_key(key(KeyCode::Char('p'))));
        assert!(view.handle_key(key(KeyCode::Char('t'))));
        assert_eq!(view.filter(), "gpt");
        assert_eq!(view.item_count(), 1);
        assert_eq!(view.state.filtered_options[0].value, "gpt-4o");
    }

    #[test]
    fn test_standard_modal_search_clear_and_close() {
        let registry = ModelRegistry::new(test_models(), "claude-sonnet-3-5".to_string());
        let mut view = StandardModalView::model(&registry);

        assert!(view.handle_key(key(KeyCode::Char('g'))));
        assert!(view.handle_key(key(KeyCode::Char('p'))));
        assert_eq!(view.filter(), "gp");

        assert!(view.handle_key(key(KeyCode::Backspace)));
        assert_eq!(view.filter(), "g");

        assert!(view.handle_key(ctrl('c')));
        assert_eq!(view.filter(), "");
        assert_eq!(view.item_count(), 2);

        assert!(view.state.is_open);
        assert!(view.handle_key(key(KeyCode::Esc)));
        assert!(!view.state.is_open);
    }

    #[test]
    fn test_auth_and_session_modal_constructors() {
        let providers = vec![ProviderDef {
            id: "anthropic",
            name: "Anthropic Claude",
            auth_mode: AuthMode::OAuth,
            default_env: "ANTHROPIC_API_KEY",
            description: "Claude models with reasoning",
        }];
        let auth_modal = StandardModalView::auth(&providers, Some("anthropic"));
        assert_eq!(auth_modal.title(), "Login Provider");
        assert!(auth_modal.state.filtered_options[0].is_active);

        let now = chrono::Utc::now();
        let sessions = vec![SessionSummary {
            session_id: "sess_12345".to_string(),
            name: Some("Refactoring UI".to_string()),
            created_at: now,
            last_modified: now,
            turn_count: 5,
            preview: "hello".to_string(),
        }];
        let session_modal = StandardModalView::session(&sessions, Some("sess_12345"));
        assert_eq!(session_modal.title(), "Resume Session");
        assert!(session_modal.state.filtered_options[0].is_active);
    }

    #[test]
    fn test_remote_pair_modal_view_rendering_and_dismiss() {
        let ticket = RhoTicket {
            node_id: "node_abc123".to_string(),
            relay_url: Some("https://relay.rho.dev".to_string()),
        };
        let mut pair_view = RemotePairModalView::new(ticket);
        assert_eq!(pair_view.title(), "Pair Remote Node");
        assert!(pair_view.is_open);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                pair_view.render(f, Rect::new(0, 0, 80, 24));
            })
            .unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Pair Remote Node"));
        assert!(text.contains("ticket:node_abc123@https://relay.rho.dev"));

        assert!(pair_view.handle_key(key(KeyCode::Esc)));
        assert!(!pair_view.is_open);
    }

    #[test]
    fn test_autocomplete_popup_anchoring_and_rendering() {
        let candidates = vec![
            AutocompleteCandidate {
                value: "/model".to_string(),
                display: "/model".to_string(),
                description: Some("Select active model".to_string()),
                score: 100,
                replacement: 0..6,
            },
            AutocompleteCandidate {
                value: "/thinking".to_string(),
                display: "/thinking".to_string(),
                description: Some("Set reasoning effort".to_string()),
                score: 90,
                replacement: 0..9,
            },
        ];
        let mut popup = AutocompletePopupView::new(candidates);
        assert_eq!(popup.selected_index, 0);

        popup.select_next();
        assert_eq!(popup.selected_index, 1);
        assert_eq!(popup.selected().unwrap().display, "/thinking");

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let cursor_area = Rect::new(10, 20, 1, 1);
                let container_area = Rect::new(0, 0, 80, 24);
                popup.render_anchored(f, cursor_area, container_area);
            })
            .unwrap();
        let text = buffer_text(terminal.backend());
        assert!(text.contains("Autocomplete"));
        assert!(text.contains("/model"));
        assert!(text.contains("/thinking"));
    }
}
