use super::ReplSession;
use super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::interactive::CompletionSet;
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
    state.set_a2ui_widgets(session.config.ui.a2ui_widgets);
    state.set_a2ui_config(session.config.ui.a2ui.clone());
    update_footer(&mut state, session, engine);
    state
}

pub(super) fn init_live_renderer(session: &mut ReplSession) -> tokio::sync::mpsc::UnboundedReceiver<UiEvent> {
    let (ui, ui_events) = crate::ui::interactive::InteractiveUi::channel();
    tokio::task::spawn_blocking(crate::ui::markdown::warmup_syntax_highlighter);
    session.renderer = TerminalRenderer::with_ui(ui);
    session.renderer.theme = crate::ui::theme::detect_with_config(&session.config.ui);
    if let Some(display) = &session.config.ui.display {
        session.renderer.set_display_config(display.clone());
    }
    ui_events
}

pub(super) fn init_live_controller(
    session: &mut ReplSession,
    engine: &AgentEngine,
) -> Result<TerminalController<CrosstermBackend>> {
    let state = init_live_state(session, engine);
    let mut controller = TerminalController::stdout(state)?;
    controller.set_approval_focus(session.config.subagents.approval_focus.unwrap_or_default());
    let _ = controller.configure_terminal_title(
        session.config.ui.terminal_title_enabled(),
        session.config.ui.terminal_title_state_enabled(),
    );
    let _ = controller.set_theme(session.renderer.theme.clone());
    session.renderer.set_width(controller.width());
    Ok(controller)
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
    let workflows = crate::repl::commands::workflows::completion_entries().await;
    session.refresh_model_catalog();
    let custom_providers = session.config.providers.keys().cloned().collect();
    let sources = crate::repl::interactive::CompletionSources::new()
        .with_skills(skills)
        .with_templates(prompt_templates)
        .with_workflows(workflows)
        .with_model_list(session.model_list())
        .with_custom_providers(custom_providers)
        .with_mcp_servers(session.mcp_completions.clone());
    CompletionSet::from_sources(sources)
}

pub(super) async fn maybe_hydrate_transcript<B: TerminalBackend>(
    resumed: bool,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
) {
    if resumed && let Ok(tree) = engine.session_manager.load_tree().await {
        let _ = hydrate_session_transcript(controller, &tree);
    }
}
