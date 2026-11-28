//! Model switching shared by the line and live frontends. The engine switches first; the session
//! config and the saved default follow only a successful switch.

use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ModelRoute;

use super::ReplSession;
use crate::engine::AgentEngine;
use crate::ui::TerminalRenderer;
use crate::ui::render::NoticeKind;

/// Switches the engine to `route` and adopts it in the session config only if that worked.
pub(crate) async fn switch_session_model(
    route: &ModelRoute,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> bool {
    let result = engine.switch_model(&route.model, &route.connection.to_string()).await;
    let switched = result.is_ok();
    if switched {
        session.config.set_route(route.clone());
    }
    report_model_update(session, engine, result, "Could not switch model");
    switched
}

/// Reports a model or thinking change: a warning when it failed, a notice when it resets a warm prompt cache.
pub(crate) fn report_model_update(session: &ReplSession, engine: &AgentEngine, result: Result<()>, failure: &str) {
    match result {
        Err(err) => session.renderer.notice(NoticeKind::Warn, &format!("{failure}: {err}")),
        Ok(()) => {
            if let Some(warning) = engine.cache_reset_warning() {
                session.renderer.notice(NoticeKind::Info, &warning);
            }
        }
    }
}

pub(crate) fn print_session_model(session: &ReplSession) {
    session.renderer.print_status(&format!(
        "Model: {} ({})",
        session.config.model, session.config.provider
    ));
}

/// The listed model `direction` steps from the current one, wrapping at both ends; `None` when no model is listed.
pub(crate) fn next_listed_route(session: &ReplSession, direction: i32) -> Option<ModelRoute> {
    let models = session.listed_models();
    if models.is_empty() {
        return None;
    }
    let current_spec = session.config.canonical_model_spec();
    let current_idx = models
        .iter()
        .position(|m| m.canonical_id() == current_spec)
        .unwrap_or(0);
    let item = &models[next_model_index(current_idx, models.len(), direction)];
    Some(ModelRoute::new(&item.provider, &item.canonical_id()))
}

fn next_model_index(current_idx: usize, len: usize, direction: i32) -> usize {
    if direction >= 0 {
        (current_idx + 1) % len
    } else if current_idx == 0 {
        len - 1
    } else {
        current_idx - 1
    }
}

/// Saves the session's current model as the default; the in-memory default follows only a successful save.
pub(crate) async fn save_default_model(config: &mut Config, renderer: &TerminalRenderer) -> bool {
    let saved = Config::save_default_model_async(&config.config_dir, &config.model, &config.provider).await;
    match saved {
        Ok(()) => {
            config.default_model = Some(config.canonical_model_spec());
            true
        }
        Err(err) => {
            renderer.notice(NoticeKind::Warn, &format!("Could not save default model: {err}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next_model_index;
    use test_case::test_case;

    #[test_case(0, 1, 1; "forward")]
    #[test_case(2, 1, 0; "forward wraps to first")]
    #[test_case(1, -1, 0; "backward")]
    #[test_case(0, -1, 2; "backward wraps to last")]
    fn cycling_wraps_at_both_ends(current: usize, direction: i32, expected: usize) {
        assert_eq!(next_model_index(current, 3, direction), expected);
    }
}
