use std::sync::Arc;

use crate::auth::AuthStore;
use rho_engine::provider::{ModelCatalog, ModelRouter};
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::thinking::ThinkingLevel;

use super::super::batch::LiveBatch;
use super::input::TurnInputContext;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) struct TurnModelSwitchInput<'a, 'b, B: TerminalBackend> {
    pub model: &'a str,
    pub provider: &'a str,
    pub save_as_default: bool,
    pub config: &'b mut Config,
    pub auth_store: &'b AuthStore,
    pub renderer: &'b TerminalRenderer,
    pub controller: &'b mut TerminalController<B>,
    pub batch: &'b mut LiveBatch,
}

async fn save_and_report_default(config: &mut Config, renderer: &TerminalRenderer, model: &str, provider: &str) {
    config.set_default_model(model, provider);
    match Config::save_default_model_async(&config.config_dir, &config.model, &config.provider).await {
        Ok(()) => renderer.print_status(&format!("Default model: {}", config.canonical_model_spec())),
        Err(err) => renderer.notice(
            crate::ui::render::NoticeKind::Warn,
            &format!("Could not save default model: {err}"),
        ),
    }
}

async fn update_config_model(
    input: &mut TurnModelSwitchInput<'_, '_, impl TerminalBackend>,
    model: &str,
    provider: &str,
    save_as_default: bool,
) {
    if save_as_default {
        save_and_report_default(input.config, input.renderer, model, provider).await;
    } else {
        input.config.set_route(ModelRoute::new(provider, model));
        input
            .renderer
            .print_status(&format!("Model: {}", input.config.canonical_model_spec()));
    }
}

async fn validate_engine_model(input: &mut TurnModelSwitchInput<'_, '_, impl TerminalBackend>) {
    let auth = Arc::new(tokio::sync::Mutex::new(input.auth_store.clone()));
    let router = ModelRouter::new(auth, ModelCatalog::default());
    let validation = router.build(input.config, &input.config.route()).await;
    if let Err(err) = validation {
        input.renderer.notice(
            crate::ui::render::NoticeKind::Warn,
            &format!("Could not switch model: {err}"),
        );
    }
}

pub(crate) async fn apply_turn_model_switch<B: TerminalBackend>(
    mut input: TurnModelSwitchInput<'_, '_, B>,
) -> Result<()> {
    let (model, provider, save_as_default) = (
        input.model.to_string(),
        input.provider.to_string(),
        input.save_as_default,
    );
    update_config_model(&mut input, &model, &provider, save_as_default).await;
    validate_engine_model(&mut input).await;
    crate::repl::live::modal::update_models_role_description(input.controller, None, &model);
    crate::repl::live::modal::update_settings_model_description(input.controller, &model);
    input.controller.state_mut().footer_mut().provider = provider;
    input.controller.state_mut().footer_mut().model = model;
    input.batch.flush(input.controller, true)?;
    Ok(())
}

fn next_cyclic_index(current_idx: usize, len: usize, direction: i32) -> usize {
    if direction >= 0 {
        (current_idx + 1) % len
    } else if current_idx == 0 {
        len - 1
    } else {
        current_idx - 1
    }
}

pub(super) async fn cycle_turn_model<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    direction: i32,
) -> Result<()> {
    let models = ctx.session.listed_models();
    if models.is_empty() {
        return Ok(());
    }
    let current_spec = ctx.session.config.canonical_model_spec();
    let current_idx = models
        .iter()
        .position(|m| m.canonical_id() == current_spec)
        .unwrap_or(0);
    let item = &models[next_cyclic_index(current_idx, models.len(), direction)];

    apply_turn_model_switch(TurnModelSwitchInput {
        model: &item.canonical_id(),
        provider: &item.provider,
        save_as_default: false,
        config: &mut ctx.session.config,
        auth_store: &ctx.session.auth_store,
        renderer: &ctx.session.renderer,
        controller: ctx.controller,
        batch: ctx.batch,
    })
    .await
}

async fn apply_thinking_cycle<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    next_level: Option<String>,
) -> Result<()> {
    ctx.session.config.thinking_level = next_level;
    let model = ctx.session.config.model.clone();
    let provider = ctx.session.config.provider.clone();
    apply_turn_model_switch(TurnModelSwitchInput {
        model: &model,
        provider: &provider,
        save_as_default: false,
        config: &mut ctx.session.config,
        auth_store: &ctx.session.auth_store,
        renderer: &ctx.session.renderer,
        controller: ctx.controller,
        batch: ctx.batch,
    })
    .await?;

    let footer = ctx.controller.state_mut().footer_mut();
    footer.thinking_level = ctx.session.config.thinking_level.clone();
    let level = ThinkingLevel::from_setting(footer.thinking_level.as_deref());
    if !level.is_auto() {
        footer.resolved_thinking_level = None;
    }
    ctx.session
        .renderer
        .print_status(&format!("Thinking: {}", level.as_str()));
    ctx.batch.flush(ctx.controller, true)
}

pub(super) async fn cycle_turn_thinking<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<()> {
    let next_level = ThinkingLevel::from_setting(ctx.session.config.thinking_level.as_deref()).next();
    apply_thinking_cycle(ctx, next_level.setting()).await
}
