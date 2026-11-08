use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::session_picker::{resume_modal, resume_options};
use crate::ui::interactive::{TerminalBackend, TerminalController};
use crate::ui::render::formatters::format_relative_time;
use rho_harness_core::session::SessionSummary;

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

fn session_row(item: &SessionSummary) -> (String, String) {
    let title = item.name.as_deref().unwrap_or(&item.preview);
    let mut parts = vec![
        format_relative_time(item.last_modified),
        format!("{} turns", item.turn_count),
    ];
    if item.name.is_some() {
        parts.push(item.preview.clone());
    }
    parts.push(item.session_id.clone());
    (title.to_string(), parts.join("\t"))
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
    use super::{open_session_selector, session_row};
    use crate::ui::interactive::InteractiveState;
    use crate::ui::interactive::TerminalController;
    use crate::ui::interactive::controller::tests::fake::FakeTerminal;
    use chrono::Utc;
    use rho_harness_core::session::SessionSummary;
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

    fn summary(name: Option<&str>) -> SessionSummary {
        SessionSummary {
            session_id: "20261108_192507_46b8ae52".into(),
            name: name.map(str::to_string),
            created_at: Utc::now(),
            last_modified: Utc::now(),
            turn_count: 4,
            preview: "fix the oauth refresh".into(),
        }
    }

    #[test]
    fn unnamed_session_is_titled_by_its_first_message() {
        let (title, desc) = session_row(&summary(None));

        assert_eq!(title, "fix the oauth refresh");
        assert!(desc.ends_with("4 turns\t20261108_192507_46b8ae52"), "{desc}");
    }

    #[test]
    fn named_session_keeps_its_name_and_shows_the_first_message() {
        let (title, desc) = session_row(&summary(Some("auth work")));

        assert_eq!(title, "auth work");
        assert!(
            desc.ends_with("4 turns\tfix the oauth refresh\t20261108_192507_46b8ae52"),
            "{desc}"
        );
    }
}
