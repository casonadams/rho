use std::sync::Arc;

use crate::auth::AuthStore;
use rho_engine::engine::builder::create_engine_model;
use rho_engine::engine::runner::{ActiveModelSwitch, SharedModelSwitch};
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;

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
    pub model_switch: &'b Arc<SharedModelSwitch>,
    pub batch: &'b mut LiveBatch,
    pub shared_auth: Option<Arc<tokio::sync::Mutex<AuthStore>>>,
}

pub(crate) async fn apply_turn_model_switch<B: TerminalBackend>(input: TurnModelSwitchInput<'_, '_, B>) -> Result<()> {
    let TurnModelSwitchInput {
        model,
        provider,
        save_as_default,
        config,
        auth_store,
        renderer,
        controller,
        model_switch,
        batch,
        shared_auth,
    } = input;

    config.model = model.to_string();
    config.provider = provider.to_string();
    let _ = rho_harness_core::state::AppState::set_last_model_async(&config.config_dir, model, Some(provider)).await;
    if save_as_default {
        config.set_default_model(model, provider);
        let _ = rho_harness_core::config::Config::save_default_model_async(&config.config_dir, model, provider).await;
        renderer.print_status(&format!("Default model: {model} ({provider})"));
    } else {
        renderer.print_status(&format!("Model: {model} ({provider})"));
    }

    match create_engine_model(config, auth_store, shared_auth) {
        Ok(handle) => {
            model_switch.switch_to(ActiveModelSwitch::new(model, provider, handle));
            controller.set_system_message(format!("[Next step will use model: {model} ({provider})]"));
        }
        Err(err) => {
            renderer.print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
        }
    }

    controller.state_mut().footer_mut().model = model.to_string();
    batch.flush(controller, true)?;
    Ok(())
}

pub(super) async fn cycle_turn_model<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    direction: i32,
) -> Result<()> {
    let models = crate::repl::interactive::discover_models(&ctx.session.config, &ctx.session.auth_store);
    if models.is_empty() {
        return Ok(());
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
    apply_turn_model_switch(TurnModelSwitchInput {
        model: &item.id,
        provider: &item.provider,
        save_as_default: false,
        config: &mut ctx.session.config,
        auth_store: &ctx.session.auth_store,
        renderer: &ctx.session.renderer,
        controller: ctx.controller,
        model_switch: ctx.model_switch,
        batch: ctx.batch,
        shared_auth: ctx.shared_auth.clone(),
    })
    .await
}

pub(super) async fn cycle_turn_thinking<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<()> {
    let levels = crate::repl::live::navigation::THINKING_LEVELS;
    let current = ctx.session.config.thinking_level.as_deref().unwrap_or("off");
    let current_idx = levels
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let next_idx = (current_idx + 1) % levels.len();
    let next_level = levels[next_idx];

    ctx.session.config.thinking_level = if next_level == "off" {
        None
    } else {
        Some(next_level.to_string())
    };

    let _ = rho_harness_core::state::AppState::set_last_thinking_level_async(
        &ctx.session.config.config_dir,
        ctx.session.config.thinking_level.as_deref(),
    )
    .await;

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
        model_switch: ctx.model_switch,
        batch: ctx.batch,
        shared_auth: ctx.shared_auth.clone(),
    })
    .await?;

    ctx.controller.state_mut().footer_mut().thinking_level = ctx.session.config.thinking_level.clone();
    ctx.session.renderer.print_status(&format!(
        "Thinking: {}",
        ctx.session.config.thinking_level.as_deref().unwrap_or("off")
    ));
    ctx.batch.flush(ctx.controller, true)?;
    Ok(())
}
