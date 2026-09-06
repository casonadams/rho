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

async fn save_and_report_default(input: (&mut Config, &TerminalRenderer), (model, provider): (&str, &str)) {
    let (config, renderer) = input;
    config.set_default_model(model, provider);
    let _ = rho_harness_core::config::Config::save_default_model_async(&config.config_dir, model, provider).await;
    renderer.print_status(&format!("Default model: {model} ({provider})"));
}

async fn update_config_model(
    input: &mut TurnModelSwitchInput<'_, '_, impl TerminalBackend>,
    (model, provider, save_as_default): (&str, &str, bool),
) {
    input.config.model = model.to_string();
    input.config.provider = provider.to_string();
    if save_as_default {
        save_and_report_default((input.config, input.renderer), (model, provider)).await;
    } else {
        input.renderer.print_status(&format!("Model: {model} ({provider})"));
    }
}

async fn switch_engine_handle(
    input: &mut TurnModelSwitchInput<'_, '_, impl TerminalBackend>,
    (model, provider): (&str, &str),
) {
    match create_engine_model(input.config, input.auth_store, input.shared_auth.clone()) {
        Ok(handle) => {
            input
                .model_switch
                .switch_to(ActiveModelSwitch::new(model, provider, handle));
            input
                .controller
                .set_system_message(format!("[Next step will use model: {model} ({provider})]"));
        }
        Err(err) => {
            input
                .renderer
                .print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
        }
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
    update_config_model(&mut input, (&model, &provider, save_as_default)).await;
    switch_engine_handle(&mut input, (&model, &provider)).await;
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
    let models = crate::repl::interactive::discover_models(&ctx.session.config, &ctx.session.auth_store);
    if models.is_empty() {
        return Ok(());
    }
    let current_model = &ctx.session.config.model;
    let current_idx = models.iter().position(|m| &m.id == current_model).unwrap_or(0);
    let item = &models[next_cyclic_index(current_idx, models.len(), direction)];

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

fn next_thinking_level(current: &str) -> Option<String> {
    let levels = crate::repl::live::navigation::THINKING_LEVELS;
    let current_idx = levels
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let next_level = levels[(current_idx + 1) % levels.len()];
    (next_level != "off").then(|| next_level.to_string())
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
    ctx.batch.flush(ctx.controller, true)
}

pub(super) async fn cycle_turn_thinking<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<()> {
    let current = ctx.session.config.thinking_level.as_deref().unwrap_or("off");
    let next_level = next_thinking_level(current);
    apply_thinking_cycle(ctx, next_level).await
}
