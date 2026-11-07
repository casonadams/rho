use super::ReplSession;
use super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{CrosstermBackend, InteractiveState, TerminalBackend, TerminalController, UiEvent};

pub(super) fn init_live_state(session: &ReplSession, engine: &AgentEngine) -> InteractiveState {
    let mut state = InteractiveState::default();
    if let Some(hide) = session.config.ui.hide_thinking {
        state.set_hide_thinking(hide);
    }
    if let Some(expanded) = session.config.ui.tools_expanded {
        state.set_tools_expanded(expanded);
    }
    update_footer(&mut state, session, engine);
    state
}

pub(super) fn init_live_ui(
    session: &mut ReplSession,
    engine: &AgentEngine,
) -> Result<(
    TerminalController<CrosstermBackend>,
    tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
)> {
    let (ui, ui_events) = crate::ui::interactive::InteractiveUi::channel();
    tokio::task::spawn_blocking(crate::ui::markdown::warmup_syntax_highlighter);
    session.renderer = TerminalRenderer::with_ui(ui);
    session.renderer.theme = crate::ui::theme::detect_with_config(&session.config.ui);
    let state = init_live_state(session, engine);
    let mut controller = TerminalController::stdout(state)?;
    let _ = controller.set_theme(session.renderer.theme.clone());
    session.renderer.set_width(controller.width());
    Ok((controller, ui_events))
}

pub(super) async fn build_completions(
    session: &ReplSession,
    skills: Vec<crate::skills::ResolvedSkill>,
) -> CompletionSet {
    let prompt_templates = rho_harness_core::prompts::discover_prompt_templates_async(
        Some(&session.config.config_dir),
        std::env::current_dir().ok().as_deref(),
    )
    .await
    .into_iter()
    .map(|t| t.metadata.name)
    .collect::<Vec<_>>();
    crate::repl::interactive::spawn_background_model_refresh(&session.config, &session.auth_store);
    let models = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let custom_providers = session.config.providers.keys().cloned().collect();
    let sources = crate::repl::interactive::CompletionSources::new()
        .with_skills(skills)
        .with_templates(prompt_templates)
        .with_models(models)
        .with_custom_providers(custom_providers);
    CompletionSet::from_sources(sources)
}

pub(super) async fn maybe_hydrate_transcript<B: TerminalBackend>(
    resumed: bool,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) {
    if resumed && let Ok(tree) = engine.session_manager.load_tree().await {
        let _ = hydrate_session_transcript(controller, &tree, history);
    }
}
