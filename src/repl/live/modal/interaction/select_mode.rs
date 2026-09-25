use crossterm::event::{KeyCode, KeyEvent};

use super::super::ModalKeyResult;
use super::types::{PendingModal, prompt_label_for};
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, InteractionResponse, ModalState, OptionLayout, TerminalBackend, TerminalController, UiAction, map_key,
    modal_body_max_scroll,
};

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
    } else if super::types::is_input_trigger(&selected_label) {
        enter_label_input(controller, &selected_label);
    } else {
        respond_selected(controller, pending, selected);
    }
}

fn handle_navigation_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
            handle_arrow_key(controller, key.code);
            true
        }
        KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            true
        }
        KeyCode::Tab if key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            true
        }
        KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            true
        }
        _ => false,
    }
}

fn handle_fallback_input<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) {
    match map_key(key) {
        InputAction::ClipboardPasteImage => {
            if let Some(text) = crate::platform::clipboard::get_text_or_image_path() {
                crate::repl::live::modal::handle_modal_paste(controller, &text);
            }
        }
        InputAction::Edit(UiAction::Insert(c)) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                insert_modal_character(modal, c);
            }
        }
        _ => {}
    }
}

fn handle_action_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<bool> {
    match key.code {
        KeyCode::Char('h') | KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('l') => {
            handle_char_key(controller, key);
        }
        KeyCode::Esc => pop_and_cancel_interaction(controller, pending),
        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            if clear_filter_or_cancel(controller, pending)? {
                return Ok(true);
            }
        }
        KeyCode::Backspace => handle_backspace(controller),
        KeyCode::Enter => handle_select_enter(controller, pending),
        _ => handle_fallback_input(controller, key),
    }
    Ok(false)
}

pub(crate) fn handle_select_mode_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    pending: &mut Option<PendingModal>,
) -> Result<ModalKeyResult> {
    if handle_navigation_key(controller, key) {
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    if handle_action_key(controller, key, pending)? {
        return Ok(ModalKeyResult::Handled);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{
        InteractionOption, InteractionPrompt, InteractionResponder, InteractiveState, UiEvent,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::io;
    use tokio::sync::oneshot;

    struct MockBackend;
    impl TerminalBackend for MockBackend {
        fn set_raw_mode(&mut self, _: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn sample_prompt() -> InteractionPrompt {
        InteractionPrompt {
            title: "Permission".into(),
            body: "Run tool?".into(),
            options: vec![
                InteractionOption {
                    label: "Allow".into(),
                    description: None,
                    input: None,
                },
                InteractionOption {
                    label: "Deny".into(),
                    description: None,
                    input: None,
                },
            ],
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: OptionLayout::Vertical,
        }
    }

    fn setup_test_modal(
        prompt: InteractionPrompt,
    ) -> (
        TerminalController<MockBackend>,
        Option<PendingModal>,
        oneshot::Receiver<InteractionResponse>,
    ) {
        let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
        let (tx, rx) = oneshot::channel();
        let mut pending = None;
        super::super::install_interaction(
            &mut controller,
            UiEvent::Interaction {
                prompt,
                responder: InteractionResponder { responder: tx },
            },
            &mut pending,
        );
        (controller, pending, rx)
    }

    #[test]
    fn test_select_mode_navigation_tab_and_backtab() {
        let (mut controller, mut pending, _rx) = setup_test_modal(sample_prompt());
        assert_eq!(controller.state().active_modal().unwrap().selected, 0);

        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, tab, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().selected, 1);

        let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, backtab, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().selected, 0);

        handle_select_mode_key(&mut controller, tab, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().selected, 1);

        let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        handle_select_mode_key(&mut controller, shift_tab, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().selected, 0);
    }

    #[test]
    fn test_select_mode_search_filtering_and_ctrl_c() {
        let (mut controller, mut pending, _rx) = setup_test_modal(sample_prompt());
        controller.state_mut().active_modal_mut().unwrap().is_searchable = true;

        let char_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, char_a, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().filter_query, "a");

        let backspace = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, backspace, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().filter_query, "");

        let char_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, char_d, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().filter_query, "d");

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        handle_select_mode_key(&mut controller, ctrl_c, &mut pending).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().filter_query, "");
        assert!(controller.state().active_modal().is_some());

        handle_select_mode_key(&mut controller, ctrl_c, &mut pending).unwrap();
        assert!(controller.state().active_modal().is_none());
        assert!(pending.is_none());
    }

    #[test]
    fn test_select_mode_allow_custom_typing() {
        let mut prompt = sample_prompt();
        prompt.allow_custom = true;
        let (mut controller, mut pending, _rx) = setup_test_modal(prompt);

        let char_z = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, char_z, &mut pending).unwrap();
        let modal = controller.state().active_modal().unwrap();
        assert!(matches!(modal.mode, crate::ui::interactive::ModalMode::Input { .. }));
        assert_eq!(modal.input.text(), "z");
    }

    #[test]
    fn test_select_mode_enter_responds() {
        let (mut controller, mut pending, mut rx) = setup_test_modal(sample_prompt());
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        handle_select_mode_key(&mut controller, enter, &mut pending).unwrap();
        assert!(controller.state().active_modal().is_none());
        assert!(pending.is_none());
        assert_eq!(rx.try_recv().unwrap(), InteractionResponse::Selected(0));
    }
}
