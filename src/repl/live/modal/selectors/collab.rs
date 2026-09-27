use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

fn format_role(role: rho_harness_core::collab::CapabilityLevel) -> &'static str {
    match role {
        rho_harness_core::collab::CapabilityLevel::Full => "co-pilot",
        rho_harness_core::collab::CapabilityLevel::ViewOnly => "spectator",
    }
}

fn format_duration(connected_at: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = chrono::Utc::now().signed_duration_since(connected_at);
    let seconds = elapsed.num_seconds().max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let mins = seconds / 60;
        let rem = seconds % 60;
        format!("{mins}m {rem:02}s")
    } else {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        format!("{hours}h {mins:02}m")
    }
}

fn build_collab_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let mut options = Vec::new();
    if let Some(ref collab) = session.collab {
        let peers = collab.peers_sync();
        for peer in peers {
            let label = format!("#{:<4}", peer.id);
            let role_str = format_role(peer.role);
            let duration = format_duration(peer.connected_at);
            let desc = format!("{role_str:<10}  {duration}  [Enter / Ctrl+K to kick]");
            options.push(ModalOption::new(label, Some(desc)));
        }
    }

    if options.is_empty() {
        options.push(ModalOption::new(
            "none",
            Some("No active peers connected (share invite with /collab link)".to_string()),
        ));
    }

    (options, 0)
}

pub fn open_collab_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let (options, initial_selection) = build_collab_options(session);
    let mut modal = ModalState::new("Active Collaborators", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn handle_collab_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let is_kick =
        key.code == KeyCode::Enter || (key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL));

    if is_kick {
        let selected = controller
            .state()
            .active_modal()
            .and_then(|m| m.selected_option())
            .cloned();
        crate::repl::live::modal::pop_and_cancel(controller)?;
        if let Some(opt) = selected {
            let raw = opt.label.trim().trim_start_matches('#');
            if let Ok(peer_id) = raw.parse::<usize>() {
                return Ok(ModalKeyResult::CollabPeerKicked { peer_id });
            }
        }
        return Ok(ModalKeyResult::Handled);
    }
    if key.code == KeyCode::Esc {
        crate::repl::live::modal::pop_and_cancel(controller)?;
        return Ok(ModalKeyResult::Handled);
    }
    crate::repl::live::modal::handle_selector_nav(controller, &key)?;
    Ok(ModalKeyResult::Handled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session() -> ReplSession {
        ReplSession::new(
            rho_harness_core::config::Config::default(),
            rho_engine::auth::AuthStore::default(),
            None,
        )
    }

    #[test]
    fn format_role_co_pilot_and_spectator() {
        assert_eq!(format_role(rho_harness_core::collab::CapabilityLevel::Full), "co-pilot");
        assert_eq!(
            format_role(rho_harness_core::collab::CapabilityLevel::ViewOnly),
            "spectator"
        );
    }

    #[test]
    fn build_collab_options_empty() {
        let session = make_test_session();
        let (opts, sel) = build_collab_options(&session);
        assert_eq!(sel, 0);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].label, "none");
    }
}
