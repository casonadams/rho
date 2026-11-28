use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::session::SessionSummary;
use tokio::sync::oneshot;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::session_picker::{resume_modal, resume_options, session_row};
use crate::ui::interactive::{ModalKind, TerminalBackend, TerminalController};

const LOADING: &str = "Loading sessions\u{2026}";

/// The saved sessions, listed off the UI thread, with any problem met while listing them.
#[derive(Debug, Default)]
pub struct ListedSessions {
    summaries: Vec<SessionSummary>,
    warnings: Vec<String>,
}

/// The session listing the open selector is waiting for.
#[derive(Debug)]
pub struct SessionListing {
    rx: oneshot::Receiver<ListedSessions>,
}

async fn list_sessions(sessions_dir: &Path, active_session_id: &str) -> ListedSessions {
    let mut listed = ListedSessions::default();
    if let Err(err) = rho_harness_core::session::prune_empty_sessions_async(sessions_dir, Some(active_session_id)).await
    {
        listed
            .warnings
            .push(format!("Warning: could not prune empty sessions: {err}"));
    }
    match rho_harness_core::session::list_session_summaries_async(sessions_dir).await {
        Ok(summaries) => listed.summaries = summaries,
        Err(err) => listed.warnings.push(format!("Warning: could not list sessions: {err}")),
    }
    listed
}

/// Opens the session selector at once, showing that it is loading, and lists the sessions in a
/// spawned task; `refresh_session_selector` fills it in when they arrive.
pub fn open_session_selector<B: TerminalBackend>(
    sessions_dir: &Path,
    active_session_id: &str,
    controller: &mut TerminalController<B>,
) -> SessionListing {
    controller
        .state_mut()
        .push_modal(resume_modal(Vec::new(), LOADING).with_kind(ModalKind::Session));
    let (tx, rx) = oneshot::channel();
    let sessions_dir = sessions_dir.to_path_buf();
    let active_session_id = active_session_id.to_string();
    tokio::spawn(async move {
        let _ = tx.send(list_sessions(&sessions_dir, &active_session_id).await);
    });
    SessionListing { rx }
}

/// Fills the open session selector once its listing arrives, keeping what was typed to filter it;
/// returns whether the screen changed. A selector closed meanwhile is left closed.
pub fn refresh_session_selector<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    listing: &mut Option<SessionListing>,
) -> bool {
    let Some(pending) = listing.as_mut() else {
        return false;
    };
    let listed = match pending.rx.try_recv() {
        Ok(listed) => listed,
        Err(oneshot::error::TryRecvError::Empty) => return false,
        Err(oneshot::error::TryRecvError::Closed) => ListedSessions {
            warnings: vec!["Warning: could not list sessions".to_string()],
            ..ListedSessions::default()
        },
    };
    *listing = None;
    fill_session_selector(controller, listed);
    true
}

fn fill_session_selector<B: TerminalBackend>(controller: &mut TerminalController<B>, listed: ListedSessions) {
    for warning in listed.warnings {
        controller.set_system_message(warning);
    }
    let Some(modal) = controller
        .state_mut()
        .active_modal_mut()
        .filter(|modal| modal.kind == ModalKind::Session)
    else {
        return;
    };
    let options = resume_options(&listed.summaries, session_row);
    let subtitle = if options.is_empty() {
        "No previous sessions to resume"
    } else {
        ""
    };
    let query = std::mem::take(&mut modal.filter_query);
    *modal = resume_modal(options, subtitle).with_kind(ModalKind::Session);
    modal.set_filter(&query);
}

/// Opens the selector and waits for its sessions, for tests that need the listing in place.
#[cfg(test)]
pub(crate) async fn open_listed_session_selector<B: TerminalBackend>(
    sessions_dir: &Path,
    active_session_id: &str,
    controller: &mut TerminalController<B>,
) {
    let listing = open_session_selector(sessions_dir, active_session_id, controller);
    let listed = listing.rx.await.unwrap_or_default();
    fill_session_selector(controller, listed);
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
        opt.value
            .clone()
            .map(|session_id| ModalKeyResult::SessionSelected { session_id })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        LOADING, ModalKeyResult, handle_session_key, open_listed_session_selector, open_session_selector,
        refresh_session_selector,
    };
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
        open_listed_session_selector(&dir, &active.session_id, &mut controller).await;
        (controller, dir)
    }

    #[tokio::test]
    async fn the_selector_opens_loading_and_fills_in_when_the_sessions_are_listed() {
        use rho_harness_core::model::ChatMessage;
        let dir = rho_test_support::TestDir::new();
        let saved = SessionManager::new_async(&dir, None).await.unwrap();
        let messages = vec![ChatMessage::user("first question"), ChatMessage::assistant("a reply")];
        saved.append_messages(&saved.session_id, messages).await.unwrap();
        let mut controller = crate::repl::test_controller();

        let mut listing = Some(open_session_selector(&dir, "active", &mut controller));
        let modal = controller.state().active_modal().unwrap();
        assert_eq!((modal.body.as_str(), modal.all_options.len()), (LOADING, 0));
        controller.state_mut().active_modal_mut().unwrap().set_filter("first");

        while !refresh_session_selector(&mut controller, &mut listing) {
            tokio::task::yield_now().await;
        }
        assert!(listing.is_none());
        let modal = controller.state().active_modal().unwrap();
        assert_eq!(modal.body, "");
        assert_eq!(modal.all_options.len(), 1);
        assert_eq!(modal.filter_query, "first", "what was typed while loading is kept");
        assert!(!refresh_session_selector(&mut controller, &mut listing));
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

    #[test]
    fn session_option_without_value_selects_nothing() {
        use crate::ui::interactive::{ModalKind, ModalOption, ModalState};

        let mut controller = crate::repl::test_controller();
        let orphan = ModalOption::new("orphan", None::<&str>);
        let modal = ModalState::new("Sessions", "", vec![orphan]).with_kind(ModalKind::Session);
        controller.state_mut().push_modal(modal);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            handle_session_key(&mut controller, enter).unwrap(),
            ModalKeyResult::Handled
        );
        assert!(controller.state().active_modal().is_none());
    }

    #[tokio::test]
    async fn opening_selector_keeps_the_active_header_only_session() {
        let dir = rho_test_support::TestDir::new();
        let session = SessionManager::new_async(&dir, None).await.unwrap();
        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        open_listed_session_selector(&dir, &session.session_id, &mut controller).await;

        assert!(session.file_path.exists());
        session
            .append_event(SessionEventKind::UserMessage, serde_json::json!({}))
            .await
            .unwrap();
    }
}
