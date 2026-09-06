use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

use super::ModalKeyResult;

fn theme_kind(is_light: bool, is_custom: bool) -> &'static str {
    if is_light {
        "light"
    } else if is_custom {
        "custom"
    } else {
        "dark"
    }
}

fn is_active_theme(session: &ReplSession, theme_name: &str) -> bool {
    theme_name == session.config.theme
        || (session.config.theme == "default" && theme_name == "ansi")
        || (session.config.theme == "ansi" && theme_name == "default")
}

fn build_theme_options(
    session: &ReplSession,
    themes: &[&crate::ui::theme::ThemeMetadata],
) -> (Vec<ModalOption>, usize) {
    let mut options = Vec::new();
    let mut initial_selection = 0;
    for (i, item) in themes.iter().enumerate() {
        let active = is_active_theme(session, &item.name);
        if active {
            initial_selection = i;
        }
        let active_mark = if active { "✓" } else { "" };
        let default_mark = if active { "active" } else { "" };
        let kind = theme_kind(item.is_light, item.is_custom);
        options.push(ModalOption::new(
            item.name.clone(),
            Some(format!("{kind}\t{active_mark}\t{default_mark}\t{}", item.description)),
        ));
    }
    (options, initial_selection)
}

pub fn open_theme_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&session.config.config_dir));
    let (options, initial_selection) = build_theme_options(session, &registry.list());
    let mut modal = ModalState::new("Select Theme", session.config.theme.clone(), options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_theme<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    Some(opt.label.clone())
}

fn restore_initial_theme<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<()> {
    let initial_theme = controller
        .state()
        .active_modal()
        .map(|m| m.body.clone())
        .unwrap_or_else(|| "default".to_string());
    controller.state_mut().pop_modal();
    let registry = crate::ui::theme::ThemeRegistry::default();
    if let Some(theme) = registry.get(&initial_theme).cloned() {
        controller.set_theme(theme)?;
    } else {
        controller.redraw()?;
    }
    Ok(())
}

fn apply_preview_theme<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<()> {
    if let Some(theme_name) = extract_selected_theme(controller)
        && theme_name != controller.theme().name
    {
        let registry = crate::ui::theme::ThemeRegistry::default();
        if let Some(theme) = registry.get(&theme_name).cloned() {
            controller.set_theme(theme)?;
            return Ok(());
        }
    }
    controller.redraw()?;
    Ok(())
}

fn apply_theme_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    character: Option<char>,
) -> Result<()> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let mut query = modal.filter_query.clone();
        if let Some(c) = character {
            query.push(c);
        } else {
            query.pop();
        }
        modal.set_filter(&query);
    }
    apply_preview_theme(controller)
}

fn clear_theme_filter<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<bool> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if !has_filter {
        return Ok(false);
    }
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.set_filter("");
    }
    apply_preview_theme(controller)?;
    Ok(true)
}

fn select_adjacent_theme<B: TerminalBackend>(controller: &mut TerminalController<B>, next: bool) -> Result<()> {
    if next {
        controller.state_mut().select_next_modal_option();
    } else {
        controller.state_mut().select_previous_modal_option();
    }
    apply_preview_theme(controller)
}

fn handle_theme_enter<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    if let Some(theme) = extract_selected_theme(controller) {
        controller.state_mut().pop_modal();
        controller.redraw()?;
        return Ok(ModalKeyResult::ThemeSelected { theme });
    }
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

pub fn handle_theme_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => select_adjacent_theme(controller, false)?,
        KeyCode::Down | KeyCode::Tab => select_adjacent_theme(controller, true)?,
        KeyCode::Enter => return handle_theme_enter(controller),
        KeyCode::Esc => restore_initial_theme(controller)?,
        KeyCode::Backspace => apply_theme_filter(controller, None)?,
        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            if clear_theme_filter(controller)? {
                return Ok(ModalKeyResult::Handled);
            }
            restore_initial_theme(controller)?;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT) =>
        {
            apply_theme_filter(controller, Some(c))?;
        }
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}
