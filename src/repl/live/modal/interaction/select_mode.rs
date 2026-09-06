use crossterm::event::{KeyCode, KeyEvent};

use super::super::ModalKeyResult;
use super::PendingModal;
use super::prompt::{is_input_trigger, prompt_label_for};
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponse, TerminalBackend, TerminalController, UiAction, map_key,
};

fn captures_typing<B: TerminalBackend>(controller: &TerminalController<B>) -> bool {
    controller
        .state()
        .active_modal()
        .is_some_and(|m| m.allow_custom || m.is_searchable)
}

fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>, pending: &mut Option<PendingModal>) {
    controller.state_mut().pop_modal();
    if let Some(pending) = pending.take() {
        let _ = pending.responder.respond(InteractionResponse::Cancelled);
    }
}

fn is_horizontal<B: TerminalBackend>(controller: &TerminalController<B>) -> bool {
    controller
        .state()
        .active_modal()
        .is_some_and(|m| m.option_layout == crate::ui::interactive::OptionLayout::Horizontal)
}

fn scroll_modal_up<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let max_scroll = crate::ui::interactive::modal_body_max_scroll(modal, &draft, (w, h));
        modal.clamp_body_scroll(max_scroll);
        modal.scroll_body_up();
    }
}

fn scroll_modal_down<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let max_scroll = crate::ui::interactive::modal_body_max_scroll(modal, &draft, (w, h));
        modal.scroll_body_down(max_scroll);
    }
}

fn handle_horizontal_char_nav<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('h') => controller.state_mut().select_previous_modal_option(),
        KeyCode::Char('l') => controller.state_mut().select_next_modal_option(),
        KeyCode::Char('k') => scroll_modal_up(controller),
        KeyCode::Char('j') => scroll_modal_down(controller),
        _ => return false,
    }
    true
}

fn handle_vertical_char_nav<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('k') => controller.state_mut().select_previous_modal_option(),
        KeyCode::Char('j') => controller.state_mut().select_next_modal_option(),
        _ => return false,
    }
    true
}

fn dispatch_nav_char<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) -> bool {
    if captures_typing(controller) {
        return false;
    }
    if is_horizontal(controller) {
        handle_horizontal_char_nav(controller, code)
    } else {
        handle_vertical_char_nav(controller, code)
    }
}

fn insert_modal_character(modal: &mut crate::ui::interactive::ModalState, c: char) {
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
    pop_and_cancel(controller, pending);
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

fn handle_plain_select_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    (key, pending): (KeyEvent, &mut Option<PendingModal>),
) -> Result<()> {
    if let InputAction::Edit(UiAction::Insert(c)) = map_key(key)
        && let Some(modal) = controller.state_mut().active_modal_mut()
    {
        insert_modal_character(modal, c);
    }
    let _ = pending;
    Ok(())
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

pub(super) fn handle_select_mode_key<B: TerminalBackend>(
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
        KeyCode::Esc => pop_and_cancel(controller, pending),
        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            if clear_filter_or_cancel(controller, pending)? {
                return Ok(ModalKeyResult::Handled);
            }
        }
        KeyCode::Backspace => handle_backspace(controller),
        KeyCode::Enter => handle_select_enter(controller, pending),
        _ => handle_plain_select_key(controller, (key, pending))?,
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
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
