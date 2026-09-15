use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};

use rho_harness_core::session::SessionSummary;
use rho_ui_core::autocomplete::{AutocompleteCandidate, THINKING_LEVEL_OPTIONS};
use rho_ui_core::modal::{McpModalState, ModalOption, ModalState, ModelRegistry, SettingsState, SkillModalState};
use rho_ui_core::permission::{PERMISSION_ACTIONS, PermissionAction, PermissionPromptState};
use rho_ui_core::session::ProviderDef;
use rho_ui_core::state::RhoTicket;

use super::editor::{EditorMode, TextAreaEditor};
use super::{ModalView, PromptEditor, TerminalComponent};

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
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(item_style)
        })
        .collect()
}

pub fn render_modal(frame: &mut Frame, state: &ModalState, area: Rect, list_state: &mut ListState) {
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
        .title(Span::styled(
            format!(" {} ", state.title),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
    let inner_area = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner_area.height == 0 || inner_area.width == 0 {
        return;
    }

    let (search_area, list_area) = if state.search_enabled {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner_area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, inner_area)
    };

    if let Some(sa) = search_area {
        let query_text = if state.filter_query.is_empty() {
            Span::styled("> Type to search...", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(format!("> {}", state.filter_query), Style::default().fg(Color::Yellow))
        };
        frame.render_widget(Paragraph::new(query_text), sa);
    }

    let items = build_modal_items(state, list_area.width as usize);
    list_state.select(Some(state.selected_index));
    let list = List::new(items);
    frame.render_stateful_widget(list, list_area, list_state);
}

#[derive(Debug, Clone)]
pub struct StandardModalView {
    pub state: ModalState,
    pub list_state: ListState,
}

impl StandardModalView {
    pub fn new(state: ModalState) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(state.selected_index));
        Self { state, list_state }
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
                let rel_time = crate::ui::render::formatters::format_relative_time(s.last_modified);
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
        match (key.code, key.modifiers) {
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
        render_modal(frame, &self.state, area, &mut list_state);
    }
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
}

pub struct PermissionPromptView {
    pub prompt: PermissionPromptState,
    pub editor: TextAreaEditor,
    pub scroll_offset: usize,
    pub resolved_action: Option<PermissionAction>,
}

impl PermissionPromptView {
    pub fn new(prompt: PermissionPromptState) -> Self {
        let cmd = prompt.command_display.clone();
        let mut editor = TextAreaEditor::new(EditorMode::Default);
        editor.set_text(&cmd);
        Self {
            prompt,
            editor,
            scroll_offset: 0,
            resolved_action: None,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.prompt.is_editing
    }

    pub fn start_editing(&mut self) {
        self.prompt.start_editing();
        self.editor.set_text(&self.prompt.edited_command);
    }

    pub fn cancel_editing(&mut self) {
        self.prompt.cancel_editing();
        self.editor.set_text(&self.prompt.command_display);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.prompt.is_editing {
            match key.code {
                KeyCode::Enter => {
                    self.prompt.set_edited_command(self.editor.text());
                    self.prompt.is_editing = false;
                    self.resolved_action = Some(PermissionAction::Edit {
                        mutated_command: self.prompt.edited_command.clone(),
                    });
                    return true;
                }
                KeyCode::Esc => {
                    self.cancel_editing();
                    return true;
                }
                _ => return self.editor.handle_key(key),
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.resolved_action = Some(PermissionAction::Deny { reason: None });
                true
            }
            (KeyCode::Left, _) | (KeyCode::Up, _) => {
                self.prompt.select_prev();
                true
            }
            (KeyCode::Right, _) | (KeyCode::Down, _) => {
                self.prompt.select_next();
                true
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.prompt.select_next();
                true
            }
            (KeyCode::BackTab, _) | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.prompt.select_prev();
                true
            }
            (KeyCode::Char('1'), KeyModifiers::NONE) => {
                self.prompt.selected_index = 0;
                true
            }
            (KeyCode::Char('2'), KeyModifiers::NONE) => {
                self.prompt.selected_index = 1;
                true
            }
            (KeyCode::Char('3'), KeyModifiers::NONE) => {
                self.prompt.selected_index = 2;
                true
            }
            (KeyCode::Char('4'), KeyModifiers::NONE) => {
                self.start_editing();
                true
            }
            (KeyCode::Enter, _) => {
                if self.prompt.selected_index == 3 {
                    self.start_editing();
                } else {
                    self.resolved_action = self.prompt.resolve();
                }
                true
            }
            (KeyCode::Backspace, _) if self.prompt.selected_index == 2 => {
                self.prompt.custom_deny_reason.pop();
                true
            }
            (KeyCode::Char(c), KeyModifiers::NONE) if self.prompt.selected_index == 2 => {
                self.prompt.custom_deny_reason.push(c);
                true
            }
            _ => false,
        }
    }

    fn render_editing(&self, frame: &mut Frame, inner: Rect) {
        let edit_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
            .split(inner);

        frame.render_widget(
            Paragraph::new("Editing tool command:").style(Style::default().fg(Color::DarkGray)),
            edit_chunks[0],
        );

        let editor_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Command Editor ");
        let editor_inner = editor_block.inner(edit_chunks[1]);
        frame.render_widget(editor_block, edit_chunks[1]);

        self.editor.render(frame, editor_inner);

        frame.render_widget(
            Paragraph::new("[Enter] Confirm Edit  ·  [Esc] Cancel").style(Style::default().fg(Color::DarkGray)),
            edit_chunks[2],
        );
    }

    fn render_prompt(&self, frame: &mut Frame, inner: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner);

        frame.render_widget(
            Paragraph::new("The model requested execution of the following command:")
                .style(Style::default().fg(Color::DarkGray)),
            chunks[0],
        );

        let cmd_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let cmd_inner = cmd_block.inner(chunks[1]);
        frame.render_widget(cmd_block, chunks[1]);

        let cmd_text = &self.prompt.command_display;
        frame.render_widget(
            Paragraph::new(cmd_text.as_str()).style(Style::default().fg(Color::White)),
            cmd_inner,
        );

        let mut action_spans = Vec::new();
        for (i, (label, _)) in PERMISSION_ACTIONS.iter().enumerate() {
            let is_sel = i == self.prompt.selected_index;
            let digit = i + 1;
            let span_style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::Rgb(30, 45, 65))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            action_spans.push(Span::styled(format!(" [{digit}] {label} "), span_style));
            action_spans.push(Span::raw("  "));
        }
        frame.render_widget(Paragraph::new(Line::from(action_spans)), chunks[2]);

        if self.prompt.selected_index == 2 {
            let reason_prompt = format!("Denial Reason: {}█", self.prompt.custom_deny_reason);
            frame.render_widget(
                Paragraph::new(reason_prompt).style(Style::default().fg(Color::Red)),
                chunks[3],
            );
        } else {
            frame.render_widget(
                Paragraph::new("[1-4] Quick Jump  ·  [Enter] Confirm  ·  [Esc] Deny")
                    .style(Style::default().fg(Color::DarkGray)),
                chunks[3],
            );
        }
    }
}

impl ModalView for PermissionPromptView {
    fn title(&self) -> &str {
        "Tool Permission Approval"
    }

    fn selected_index(&self) -> usize {
        self.prompt.selected_index
    }

    fn item_count(&self) -> usize {
        PERMISSION_ACTIONS.len()
    }

    fn filter(&self) -> &str {
        &self.prompt.custom_deny_reason
    }

    fn set_filter(&mut self, query: &str) {
        self.prompt.set_deny_reason(query);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.handle_key(key)
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let popup_w = (area.width * 8 / 10).clamp(50, 95);
        let popup_h = (area.height * 7 / 10).clamp(10, 24);
        let popup_area = centered_modal_area(popup_w, popup_h, area);

        frame.render_widget(Clear, popup_area);

        let border_color = if self.prompt.is_editing {
            Color::Yellow
        } else {
            Color::Cyan
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" Tool Approval: {} ", self.prompt.tool_name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 4 || inner.width < 10 {
            return;
        }

        if self.prompt.is_editing {
            self.render_editing(frame, inner);
        } else {
            self.render_prompt(frame, inner);
        }
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
                    Style::default()
                };

                ListItem::new(Line::from(spans)).style(item_style)
            })
            .collect();

        let list = List::new(items);
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
    fn test_permission_prompt_navigation_and_deny_reason() {
        let prompt_state =
            PermissionPromptState::new("bash", "cargo test", serde_json::json!({"command": "cargo test"}));
        let mut prompt_view = PermissionPromptView::new(prompt_state);

        assert!(!prompt_view.is_editing());
        assert_eq!(prompt_view.selected_index(), 0);

        assert!(prompt_view.handle_key(key(KeyCode::Right)));
        assert_eq!(prompt_view.selected_index(), 1);

        assert!(prompt_view.handle_key(key(KeyCode::Char('3'))));
        assert_eq!(prompt_view.selected_index(), 2);
        assert!(prompt_view.handle_key(key(KeyCode::Char('n'))));
        assert!(prompt_view.handle_key(key(KeyCode::Char('o'))));
        assert_eq!(prompt_view.prompt.custom_deny_reason, "no");
    }

    #[test]
    fn test_permission_prompt_edit_flow() {
        let prompt_state =
            PermissionPromptState::new("bash", "cargo test", serde_json::json!({"command": "cargo test"}));
        let mut prompt_view = PermissionPromptView::new(prompt_state);

        assert!(prompt_view.handle_key(key(KeyCode::Char('4'))));
        assert!(prompt_view.is_editing());

        assert!(prompt_view.handle_key(key(KeyCode::Char(' '))));
        assert!(prompt_view.handle_key(key(KeyCode::Char('-'))));
        assert!(prompt_view.handle_key(key(KeyCode::Char('q'))));
        assert_eq!(prompt_view.editor.text(), "cargo test -q");

        assert!(prompt_view.handle_key(key(KeyCode::Enter)));
        assert!(!prompt_view.is_editing());
        assert_eq!(
            prompt_view.resolved_action,
            Some(PermissionAction::Edit {
                mutated_command: "cargo test -q".to_string()
            })
        );
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
