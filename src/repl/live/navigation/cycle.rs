use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::ui::interactive::{Activity, InteractiveState, TerminalBackend, TerminalController};

pub const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn update_footer(state: &mut InteractiveState, session: &ReplSession, engine: &AgentEngine) {
    let footer = state.footer_mut();
    footer.activity = Activity::Idle;
    footer.model = session.config.model.clone();
    footer.thinking_level = session.config.thinking_level.clone();

    let cwd = std::env::current_dir().unwrap_or_default();
    footer.cwd = Some(cwd.display().to_string());
    footer.git_branch = crate::ui::interactive::footer::get_git_branch(&cwd);
    footer.session_name = engine.session_manager.cached_session_name();
    footer.quota = engine.quota_display();
    footer.context_percent = engine.context_percent_f64();
    footer.context_window = engine.context_limit().unwrap_or(0);

    let totals = engine.session_usage_totals();
    footer.total_input_tokens = totals.total_input;
    footer.total_output_tokens = totals.total_output;
    footer.total_cache_read_tokens = totals.total_cache_read;
    footer.total_cache_write_tokens = totals.total_cache_write;
    footer.tokens_per_second = engine.tokens_per_second();
    footer.context = Some(engine.context_remaining_display());
    footer.show_label = session.config.show_label;
}

pub async fn cycle_thinking_level<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    controller: &mut TerminalController<B>,
) {
    let current = session.config.thinking_level.as_deref().unwrap_or("off");
    let current_idx = THINKING_LEVELS
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let next_idx = (current_idx + 1) % THINKING_LEVELS.len();
    let next_level = THINKING_LEVELS[next_idx];

    session.config.thinking_level = if next_level == "off" {
        None
    } else {
        Some(next_level.to_string())
    };

    let _ = rho_harness_core::state::AppState::set_last_thinking_level(
        &session.config.config_dir,
        session.config.thinking_level.as_deref(),
    );

    // Providers route on the thinking level (Antigravity runtime variants), so
    // rebuild the engine like a model switch does.
    if let Ok(rebuilt) = engine.rebuild(session.config.clone(), session.auth_store.clone()).await {
        *engine = rebuilt;
    }

    update_footer(controller.state_mut(), session, engine);
    session.renderer.print_status(&format!(
        "Thinking: {}",
        session.config.thinking_level.as_deref().unwrap_or("off")
    ));
}

pub struct ModelCycleContext<'a, 'b, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'a mut TerminalController<B>,
}

pub async fn cycle_model<B: TerminalBackend>(ctx: &mut ModelCycleContext<'_, '_, B>, direction: i32) {
    let models = crate::repl::interactive::discover_models(&ctx.session.config, &ctx.session.auth_store);
    if models.is_empty() {
        return;
    }
    let current_model = &ctx.session.config.model;
    let current_idx = models.iter().position(|m| &m.id == current_model).unwrap_or(0);

    let next_idx = if direction >= 0 {
        (current_idx + 1) % models.len()
    } else if current_idx == 0 {
        models.len() - 1
    } else {
        current_idx - 1
    };

    let item = &models[next_idx];
    ctx.session.config.model = item.id.clone();
    ctx.session.config.provider = item.provider.clone();

    let _ = rho_harness_core::state::AppState::set_last_model(
        &ctx.session.config.config_dir,
        &item.id,
        Some(&item.provider),
    );

    if let Ok(rebuilt) = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await
    {
        *ctx.engine = rebuilt;
    }

    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.session.renderer.print_status(&format!(
        "Model: {} ({})",
        ctx.session.config.model, ctx.session.config.provider
    ));
}
