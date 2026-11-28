use std::sync::Arc;

use crate::auth::AuthStore;
use crate::repl::model_switch::{next_listed_route, save_default_model};
use rho_engine::provider::{ModelCatalog, ModelRouter};
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::thinking::ThinkingLevel;

use super::super::batch::LiveBatch;
use super::input::TurnInputContext;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use crate::ui::render::NoticeKind;

pub(crate) struct TurnModelSwitchInput<'b, B: TerminalBackend> {
    pub route: ModelRoute,
    pub save_as_default: bool,
    pub config: &'b mut Config,
    pub auth_store: &'b AuthStore,
    pub renderer: &'b TerminalRenderer,
    pub controller: &'b mut TerminalController<B>,
    pub batch: &'b mut LiveBatch,
}

/// Checks that `route` builds, so a mid-turn switch changes nothing when it would fail.
async fn validate_turn_route(config: &Config, auth_store: &AuthStore, route: &ModelRoute) -> Result<()> {
    let auth = Arc::new(tokio::sync::Mutex::new(auth_store.clone()));
    ModelRouter::new(auth, ModelCatalog::default())
        .build(config, route)
        .await?;
    Ok(())
}

async fn commit_turn_route<B: TerminalBackend>(input: &mut TurnModelSwitchInput<'_, B>) {
    input.config.set_route(input.route.clone());
    if !input.save_as_default {
        input
            .renderer
            .print_status(&format!("Model: {}", input.config.canonical_model_spec()));
    } else if save_default_model(input.config, input.renderer).await {
        input
            .renderer
            .print_status(&format!("Default model: {}", input.config.canonical_model_spec()));
    }
}

pub(crate) async fn apply_turn_model_switch<B: TerminalBackend>(mut input: TurnModelSwitchInput<'_, B>) -> Result<()> {
    if let Err(err) = validate_turn_route(input.config, input.auth_store, &input.route).await {
        input
            .renderer
            .notice(NoticeKind::Warn, &format!("Could not switch model: {err}"));
        return Ok(());
    }
    commit_turn_route(&mut input).await;
    crate::repl::live::modal::update_models_role_description(input.controller, None, &input.route.model);
    crate::repl::live::modal::update_settings_model_description(input.controller, &input.route.model);
    let footer = input.controller.state_mut().footer_mut();
    footer.provider = input.config.provider.clone();
    footer.model = input.config.model.clone();
    input.batch.flush(input.controller, true)?;
    Ok(())
}

pub(super) async fn cycle_turn_model<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    direction: i32,
) -> Result<()> {
    let Some(route) = next_listed_route(ctx.session, direction) else {
        return Ok(());
    };
    apply_turn_model_switch(TurnModelSwitchInput {
        route,
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
    let route = ModelRoute::new(&ctx.session.config.provider, &ctx.session.config.model);
    apply_turn_model_switch(TurnModelSwitchInput {
        route,
        save_as_default: false,
        config: &mut ctx.session.config,
        auth_store: &ctx.session.auth_store,
        renderer: &ctx.session.renderer,
        controller: ctx.controller,
        batch: ctx.batch,
    })
    .await?;

    ctx.controller.state_mut().footer_mut().thinking_level = ctx.session.config.thinking_level.clone();
    let level = ThinkingLevel::from_setting(ctx.session.config.thinking_level.as_deref());
    ctx.session
        .renderer
        .print_status(&format!("Thinking: {}", level.as_str()));
    ctx.batch.flush(ctx.controller, true)
}

pub(super) async fn cycle_turn_thinking<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<()> {
    let next_level = ThinkingLevel::from_setting(ctx.session.config.thinking_level.as_deref()).next();
    apply_thinking_cycle(ctx, next_level.setting()).await
}
