use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::session_picker::{resume_modal, resume_options, session_row};
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub async fn open_session_selector<B: TerminalBackend>(
    sessions_dir: &Path,
    active_session_id: &str,
    controller: &mut TerminalController<B>,
) {
    let _ = rho_harness_core::session::prune_empty_sessions_async(sessions_dir, Some(active_session_id)).await;
    let summaries = rho_harness_core::session::list_session_summaries_async(sessions_dir)
        .await
        .unwrap_or_default();
    let options = resume_options(&summaries, session_row);
    let subtitle = if options.is_empty() {
        "No previous sessions to resume"
    } else {
        ""
    };
    controller.state_mut().push_modal(resume_modal(options, subtitle));
}

fn selected_session_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    controller.state().active_modal()?.selected_option()?.value.clone()
}

fn delete_selected_session<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let Some(session_id) = selected_session_id(controller) else {
        return Ok(ModalKeyResult::Handled);
    };
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
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        let session_id = opt.value.clone().unwrap_or_default();
        Some(ModalKeyResult::SessionSelected { session_id })
    })
}

#[cfg(test)]
mod tests {
    use super::open_session_selector;
    use crate::ui::interactive::InteractiveState;
    use crate::ui::interactive::TerminalController;
    use crate::ui::interactive::controller::tests::fake::FakeTerminal;
    use rho_harness_core::session::{SessionEventKind, SessionManager};

    #[tokio::test]
    async fn opening_selector_keeps_the_active_header_only_session() {
        let dir = std::env::temp_dir().join(format!("rho_selector_prune_{}", uuid::Uuid::new_v4()));
        let session = SessionManager::new_async(&dir, None).await.unwrap();
        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        open_session_selector(&dir, &session.session_id, &mut controller).await;

        assert!(session.file_path.exists());
        session
            .append_event(SessionEventKind::UserMessage, serde_json::json!({}))
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
