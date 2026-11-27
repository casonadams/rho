use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::session_picker::{resume_modal, resume_options, session_row};
use crate::ui::interactive::{ModalKind, TerminalBackend, TerminalController};

pub async fn open_session_selector<B: TerminalBackend>(
    sessions_dir: &Path,
    active_session_id: &str,
    controller: &mut TerminalController<B>,
) {
    if let Err(err) = rho_harness_core::session::prune_empty_sessions_async(sessions_dir, Some(active_session_id)).await
    {
        controller.set_system_message(format!("Warning: could not prune empty sessions: {err}"));
    }
    let summaries = match rho_harness_core::session::list_session_summaries_async(sessions_dir).await {
        Ok(summaries) => summaries,
        Err(err) => {
            controller.set_system_message(format!("Warning: could not list sessions: {err}"));
            Vec::new()
        }
    };
    let options = resume_options(&summaries, session_row);
    let subtitle = if options.is_empty() {
        "No previous sessions to resume"
    } else {
        ""
    };
    controller
        .state_mut()
        .push_modal(resume_modal(options, subtitle).with_kind(ModalKind::Session));
}

fn selected_session_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    controller.state().active_modal()?.selected_option()?.value.clone()
}

fn delete_selected_session<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let Some(session_id) = selected_session_id(controller) else {
        return Ok(ModalKeyResult::Handled);
    };
    let confirmed = controller
        .state_mut()
        .active_modal_mut()
        .is_some_and(|modal| modal.pending_delete.take().as_deref() == Some(session_id.as_str()));
    if !confirmed {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.pending_delete = Some(session_id);
        }
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal
            .all_options
            .retain(|o| o.value.as_deref() != Some(session_id.as_str()));
        let q = modal.filter_query.clone();
        modal.set_filter(&q);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::SessionDeleted { session_id })
}

pub fn handle_session_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return delete_selected_session(controller);
    }
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.pending_delete = None;
    }
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        let session_id = opt.value.clone().unwrap_or_default();
        Some(ModalKeyResult::SessionSelected { session_id })
    })
}

#[cfg(test)]
mod tests {
    use super::{ModalKeyResult, handle_session_key, open_session_selector};
    use crate::ui::interactive::InteractiveState;
    use crate::ui::interactive::TerminalController;
    use crate::ui::interactive::controller::tests::fake::FakeTerminal;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rho_harness_core::session::{SessionEventKind, SessionManager};

    const CTRL_D: KeyEvent = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);

    async fn selector_over_two_sessions() -> (TerminalController<FakeTerminal>, rho_test_support::TestDir) {
        use rho_harness_core::model::ChatMessage;
        let dir = rho_test_support::TestDir::new();
        for prompt in ["first question", "second question"] {
            let saved = SessionManager::new_async(&dir, None).await.unwrap();
            let messages = vec![ChatMessage::user(prompt), ChatMessage::assistant("a reply")];
            saved.append_messages(&saved.session_id, messages).await.unwrap();
        }
        let active = SessionManager::new_async(&dir, None).await.unwrap();
        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        open_session_selector(&dir, &active.session_id, &mut controller).await;
        (controller, dir)
    }

    fn selected_id(controller: &TerminalController<FakeTerminal>) -> String {
        let modal = controller.state().active_modal().unwrap();
        modal.selected_option().unwrap().value.clone().unwrap()
    }

    #[tokio::test]
    async fn ctrl_d_needs_a_second_press_to_delete_the_selected_session() {
        let (mut controller, _dir) = selector_over_two_sessions().await;
        let session_id = selected_id(&controller);

        let first = handle_session_key(&mut controller, CTRL_D).unwrap();
        assert!(matches!(first, ModalKeyResult::Handled));
        let modal = controller.state().active_modal().unwrap();
        assert_eq!(modal.pending_delete.as_deref(), Some(session_id.as_str()));
        assert_eq!(modal.all_options.len(), 2);

        let second = handle_session_key(&mut controller, CTRL_D).unwrap();
        assert!(matches!(second, ModalKeyResult::SessionDeleted { session_id: id } if id == session_id));
        let modal = controller.state().active_modal().unwrap();
        assert!(modal.pending_delete.is_none());
        assert_eq!(modal.all_options.len(), 1);
        assert_ne!(selected_id(&controller), session_id);
    }

    #[tokio::test]
    async fn any_other_key_cancels_a_pending_delete() {
        let (mut controller, _dir) = selector_over_two_sessions().await;
        handle_session_key(&mut controller, CTRL_D).unwrap();

        handle_session_key(&mut controller, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).unwrap();
        assert!(controller.state().active_modal().unwrap().pending_delete.is_none());

        let again = handle_session_key(&mut controller, CTRL_D).unwrap();
        assert!(matches!(again, ModalKeyResult::Handled));
        assert_eq!(controller.state().active_modal().unwrap().all_options.len(), 2);
    }

    #[tokio::test]
    async fn opening_selector_keeps_the_active_header_only_session() {
        let dir = rho_test_support::TestDir::new();
        let session = SessionManager::new_async(&dir, None).await.unwrap();
        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        open_session_selector(&dir, &session.session_id, &mut controller).await;

        assert!(session.file_path.exists());
        session
            .append_event(SessionEventKind::UserMessage, serde_json::json!({}))
            .await
            .unwrap();
    }
}
