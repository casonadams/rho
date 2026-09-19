use crossterm::event::{KeyCode, KeyEvent};

use super::{ModalKeyResult, apply_input_edit};
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionPrompt, InteractionResponder, InteractionResponse, ModalMode, ModalOption, ModalState,
    OptionLayout, TerminalBackend, TerminalController, UiAction, UiEvent, map_key, modal_body_max_scroll,
};

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
}

fn is_input_trigger(label: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "with reason",
        "with feedback",
        "custom answer",
        "custom input",
        "Type something",
        "Type a custom",
        "Deny with reason",
        "Accept input",
    ];
    PATTERNS.iter().any(|p| label.contains(p))
}

pub(super) fn prompt_label_for(label: &str) -> &'static str {
    if label.contains("reason")
        || label.contains("feedback")
        || label.contains("Permission")
        || label.contains("Approve")
    {
        "reason"
    } else {
        "answer"
    }
}

fn build_interaction_state(prompt: InteractionPrompt) -> ModalState {
    let options = prompt
        .options
        .into_iter()
        .map(|o| ModalOption {
            label: o.label,
            description: o.description,
            input: o.input,
        })
        .collect::<Vec<_>>();
    let is_empty = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options)
        .with_custom(prompt.allow_custom)
        .with_option_layout(prompt.option_layout);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty || (prompt.allow_custom && state.options.is_empty()) || prompt.initial_text.is_some() {
        state.enter_input_mode("input");
    }
    if let Some(prefill) = prompt.initial_text {
        state.input.set_text(prefill);
    }
    state
}

pub fn install_interaction<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: UiEvent,
    modal: &mut Option<PendingModal>,
) {
    let UiEvent::Interaction { prompt, responder } = event else {
        unreachable!("only interaction events create ordered barriers");
    };
    controller.state_mut().push_modal(build_interaction_state(prompt));
    *modal = Some(PendingModal { responder });
}

pub fn handle_interaction_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(ModalKeyResult::NotHandled);
    };

    match &active.mode {
        ModalMode::Input { .. } => handle_input_mode_key(controller, key, pending),
        ModalMode::Select => handle_select_mode_key(controller, key, pending),
    }
}

fn exit_input_or_pop_modal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
) {
    let has_options = controller.state().active_modal().is_some_and(|m| !m.options.is_empty());
    if has_options {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.exit_input_mode();
        }
        return;
    }
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let _ = pending.responder.respond(InteractionResponse::Cancelled);
    }
}

fn build_enter_response(custom: String, input_option: Option<usize>) -> InteractionResponse {
    if let Some(index) = input_option {
        InteractionResponse::SelectedWithInput { index, text: custom }
    } else if !custom.is_empty() {
        InteractionResponse::Custom(custom)
    } else {
        InteractionResponse::Cancelled
    }
}

fn submit_enter<B: TerminalBackend>(controller: &mut TerminalController<B>, pending: &mut Option<PendingModal>) {
    let custom = controller
        .state()
        .active_modal()
        .map(|m| m.input.expanded_text().trim().to_string())
        .unwrap_or_default();
    let input_option = controller.state().active_modal().and_then(|m| m.input_option);
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let response = build_enter_response(custom, input_option);
        let _ = pending.responder.respond(response);
    }
}

fn apply_modal_edit<B: TerminalBackend>(controller: &mut TerminalController<B>, edit: UiAction) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        apply_input_edit(&mut modal.input, edit);
    }
}

fn handle_vertical_move<B: TerminalBackend>(controller: &mut TerminalController<B>, down: bool) {
    let width = controller.terminal_width().saturating_sub(2).max(1);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        if down {
            modal.input.move_down(width);
        } else {
            modal.input.move_up(width);
        }
    }
}

fn handle_plain_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) {
    match map_key(key) {
        InputAction::Clear => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.input.set_text("");
            }
        }
        InputAction::HistoryPrevious => handle_vertical_move(controller, false),
        InputAction::HistoryNext => handle_vertical_move(controller, true),
        InputAction::ClipboardPasteImage => {
            if let Some(text) = crate::platform::clipboard::get_text_or_image_path() {
                apply_modal_edit(controller, UiAction::Paste(text));
            }
        }
        InputAction::Edit(action) => apply_modal_edit(controller, action),
        _ => {}
    }
}

fn is_newline_key(key: &KeyEvent) -> bool {
    (key.code == KeyCode::Enter
        && key.modifiers.intersects(
            crossterm::event::KeyModifiers::SHIFT
                | crossterm::event::KeyModifiers::ALT
                | crossterm::event::KeyModifiers::CONTROL,
        ))
        || (key.code == KeyCode::Char('j') && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL))
}

fn handle_input_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Esc {
        exit_input_or_pop_modal(controller, pending);
    } else if is_newline_key(&key) {
        apply_modal_edit(controller, UiAction::InsertNewline);
    } else if key.code == KeyCode::Enter {
        submit_enter(controller, pending);
    } else {
        handle_plain_key(controller, key);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn captures_typing<B: TerminalBackend>(controller: &TerminalController<B>) -> bool {
    controller
        .state()
        .active_modal()
        .is_some_and(|m| m.allow_custom || m.is_searchable)
}

fn pop_and_cancel_interaction<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
) {
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let _ = pending.responder.respond(InteractionResponse::Cancelled);
    }
}

fn is_horizontal<B: TerminalBackend>(controller: &TerminalController<B>) -> bool {
    controller
        .state()
        .active_modal()
        .is_some_and(|m| m.option_layout == OptionLayout::Horizontal)
}

fn scroll_modal_up<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let max_scroll = modal_body_max_scroll(modal, &draft, w, h);
        modal.clamp_body_scroll(max_scroll);
        modal.scroll_body_up();
    }
}

fn scroll_modal_down<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let max_scroll = modal_body_max_scroll(modal, &draft, w, h);
        modal.scroll_body_down(max_scroll);
    }
}

fn dispatch_nav_char<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) -> bool {
    if captures_typing(controller) {
        return false;
    }
    if is_horizontal(controller) {
        match code {
            KeyCode::Char('h') => controller.state_mut().select_previous_modal_option(),
            KeyCode::Char('l') => controller.state_mut().select_next_modal_option(),
            KeyCode::Char('k') => scroll_modal_up(controller),
            KeyCode::Char('j') => scroll_modal_down(controller),
            _ => return false,
        }
    } else {
        match code {
            KeyCode::Char('k') => controller.state_mut().select_previous_modal_option(),
            KeyCode::Char('j') => controller.state_mut().select_next_modal_option(),
            _ => return false,
        }
    }
    true
}

fn insert_modal_character(modal: &mut ModalState, c: char) {
    if modal.is_searchable {
        let mut query = modal.filter_query.clone();
        query.push(c);
        modal.set_filter(&query);
    } else if modal.allow_custom {
        let prompt = prompt_label_for(&modal.title);
        modal.enter_input_mode(prompt);
        modal.input.insert(c);
    }
}

fn handle_char_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) {
    if dispatch_nav_char(controller, key.code) {
        return;
    }
    if let InputAction::Edit(UiAction::Insert(c)) = map_key(key)
        && let Some(modal) = controller.state_mut().active_modal_mut()
    {
        insert_modal_character(modal, c);
    }
}

fn clear_filter_or_cancel<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
) -> Result<bool> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw()?;
        return Ok(true);
    }
    pop_and_cancel_interaction(controller, pending);
    Ok(false)
}

fn handle_backspace<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    if let Some(modal) = controller.state_mut().active_modal_mut()
        && modal.is_searchable
    {
        let mut query = modal.filter_query.clone();
        query.pop();
        modal.set_filter(&query);
    }
}

fn handle_arrow_key<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) {
    if is_horizontal(controller) {
        match code {
            KeyCode::Left => controller.state_mut().select_previous_modal_option(),
            KeyCode::Right => controller.state_mut().select_next_modal_option(),
            KeyCode::Up => scroll_modal_up(controller),
            KeyCode::Down => scroll_modal_down(controller),
            _ => {}
        }
    } else {
        match code {
            KeyCode::Up => controller.state_mut().select_previous_modal_option(),
            KeyCode::Down => controller.state_mut().select_next_modal_option(),
            _ => {}
        }
    }
}

fn select_option_input<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (selected, spec): (usize, crate::ui::interactive::InteractionInput),
) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.selected = selected;
        modal.input_option = Some(selected);
        modal.enter_input_mode(&spec.label);
        if let Some(prefill) = spec.value {
            modal.input.set_text(prefill);
        }
    }
}

fn enter_label_input<B: TerminalBackend>(controller: &mut TerminalController<B>, selected_label: &str) {
    let prompt = prompt_label_for(selected_label);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.enter_input_mode(prompt);
    }
}

fn respond_selected<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    pending: &mut Option<PendingModal>,
    selected: usize,
) {
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let _ = pending.responder.respond(InteractionResponse::Selected(selected));
    }
}

fn handle_select_enter<B: TerminalBackend>(controller: &mut TerminalController<B>, pending: &mut Option<PendingModal>) {
    let selected = controller.state().active_modal().map_or(0, |modal| modal.selected);
    let selected_label = controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .map(|opt| opt.label.clone())
        .unwrap_or_default();
    let option_input = controller
        .state()
        .active_modal()
        .and_then(|m| m.options.get(selected))
        .and_then(|opt| opt.input.clone());

    if let Some(spec) = option_input {
        select_option_input(controller, (selected, spec));
    } else if is_input_trigger(&selected_label) {
        enter_label_input(controller, &selected_label);
    } else {
        respond_selected(controller, pending, selected);
    }
}

fn handle_select_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => handle_arrow_key(controller, key.code),
        KeyCode::BackTab => controller.state_mut().select_previous_modal_option(),
        KeyCode::Tab if key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
        }
        KeyCode::Tab => controller.state_mut().select_next_modal_option(),
        KeyCode::Char('h') | KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
            handle_char_key(controller, key);
        }
        KeyCode::Esc => pop_and_cancel_interaction(controller, pending),
        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            if clear_filter_or_cancel(controller, pending)? {
                return Ok(ModalKeyResult::Handled);
            }
        }
        KeyCode::Backspace => handle_backspace(controller),
        KeyCode::Enter => handle_select_enter(controller, pending),
        _ => match map_key(key) {
            InputAction::ClipboardPasteImage => {
                if let Some(text) = crate::platform::clipboard::get_text_or_image_path() {
                    super::handle_modal_paste(controller, &text);
                }
            }
            InputAction::Edit(UiAction::Insert(c)) => {
                if let Some(modal) = controller.state_mut().active_modal_mut() {
                    insert_modal_character(modal, c);
                }
            }
            _ => {}
        },
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}
