use super::ReplSession;
use super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{CrosstermBackend, InteractiveState, TerminalBackend, TerminalController, UiEvent};
use crate::ui::render::WelcomeDisplay;

pub(super) async fn init_live_engine(session: &mut ReplSession) -> Result<AgentEngine> {
    let engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;
    if let Some(ref cli) = session.cli
        && let Some(ref name) = cli.name
    {
        let _ = engine.session_manager.set_session_name(name).await;
    }
    session.config = engine.config.clone();
    engine.refresh_quota().await;
    Ok(engine)
}

pub(super) fn init_live_ui(
    session: &mut ReplSession,
    engine: &AgentEngine,
) -> Result<(
    TerminalController<CrosstermBackend>,
    tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
)> {
    let (ui, ui_events) = crate::ui::interactive::InteractiveUi::channel();
    session.renderer = TerminalRenderer::with_ui(ui);
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&session.config.config_dir));
    if let Some(initial_theme) = registry.get(&session.config.theme).cloned() {
        session.renderer.theme = initial_theme;
    }
    let mut state = InteractiveState::default();
    update_footer(&mut state, session, engine);
    let mut controller = TerminalController::stdout(state)?;
    if let Some(initial_theme) = registry.get(&session.config.theme).cloned() {
        let _ = controller.set_theme(initial_theme);
    }
    Ok((controller, ui_events))
}

pub(super) async fn display_welcome_banner(
    session: &ReplSession,
    engine: &AgentEngine,
) -> Vec<crate::skills::ResolvedSkill> {
    let skills =
        tokio::task::spawn_blocking(|| crate::skills::resolved_skills(std::env::current_dir().ok().as_deref()))
            .await
            .unwrap_or_default();
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let tools = engine.tool_names();
    let mut plugins = session.config.plugins.keys().cloned().collect::<Vec<_>>();
    for mcp in session.config.mcp.servers.keys() {
        if !plugins.contains(mcp) {
            plugins.push(mcp.clone());
        }
    }
    let agents = engine.instruction_files().await;
    session.renderer.print_welcome(&WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        plugins,
    });
    skills
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
    (controller, history): (&mut TerminalController<B>, &mut InteractiveHistory),
) {
    if resumed && let Ok(tree) = engine.session_manager.load_tree().await {
        let _ = hydrate_session_transcript(controller, &tree, history);
    }
}
