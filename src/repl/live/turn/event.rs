use crossterm::event::{Event, KeyEvent, KeyEventKind};

use super::cancel::cancel_active_turn;
use super::input::{TurnInputContext, TurnKeyResult, handle_turn_key};
use super::model_switch::{TurnModelSwitchInput, apply_turn_model_switch};
use super::runner::TurnLoop;
use crate::engine::runner::CancellationSignal;
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::repl::live::agent_focus;
use crate::repl::live::autocomplete::{AutocompleteKeyResult, handle_autocomplete_key, update_autocomplete_state};
use crate::repl::live::modal::actions::{apply_shared_modal_action, open_shared_help_modal, save_or_print_thinking};
use crate::repl::live::modal::{ModalKeyResult, agents, handle_modal_key, handle_modal_paste};
use crate::ui::interactive::{Activity, TerminalBackend, UiAction};

pub(super) struct TurnInputResources<'a> {
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
    pub ui_events: &'a mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    pub cancellation: &'a CancellationSignal,
}

fn handle_resize<B: TerminalBackend>(lp: &mut TurnLoop<'_, B>, cols: u16, rows: u16) -> Result<()> {
    let resized = lp.controller.resize_to(usize::from(cols), usize::from(rows))? || lp.controller.refresh_size()?;
    if resized {
        lp.session.renderer.set_width(lp.controller.width());
    }
    lp.batch.flush(lp.controller, true)
}

fn handle_paste<B: TerminalBackend>(lp: &mut TurnLoop<'_, B>, completions: &CompletionSet, text: String) -> Result<()> {
    if !handle_modal_paste(lp.controller, &text) {
        lp.controller.state_mut().apply(UiAction::Paste(text));
        update_autocomplete_state(lp.controller, completions);
    }
    lp.batch.flush(lp.controller, true)
}

fn handle_focus_change<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
    focused: bool,
) -> Result<()> {
    if lp.controller.focused() == focused {
        return Ok(());
    }
    lp.controller.set_focused(focused);
    let _ = lp.drain_ui_batch(ui_events, false);
    if matches!(lp.controller.state().footer().activity, Activity::Idle) {
        lp.controller.state_mut().footer_mut().activity = Activity::Working;
    }
    lp.batch.flush(lp.controller, true)
}

pub(super) async fn dispatch_turn_input<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    res: &mut TurnInputResources<'_>,
    event: Event,
) -> Result<bool> {
    match event {
        Event::Resize(cols, rows) => {
            handle_resize(lp, cols, rows)?;
            Ok(false)
        }
        Event::Paste(text) => {
            handle_paste(lp, res.completions, text)?;
            Ok(false)
        }
        Event::FocusGained => {
            handle_focus_change(lp, res.ui_events, true)?;
            Ok(false)
        }
        Event::FocusLost => {
            handle_focus_change(lp, res.ui_events, false)?;
            Ok(false)
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => dispatch_key_event(lp, res, key).await,
        _ => Ok(false),
    }
}

async fn dispatch_key_event<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    res: &mut TurnInputResources<'_>,
    key: KeyEvent,
) -> Result<bool> {
    let monitor = lp.engine.agent_monitor();
    if let Some(text) = agent_focus::take_submission(lp.controller, &key) {
        crate::repl::live::navigation::record_submission(res.history, &text)?;
        if let agent_focus::Routed::ContinueThroughMain(prompt) = agent_focus::route(lp.controller, lp.engine, &text)
            && let Err(full) = lp.steering.enqueue(prompt)
        {
            lp.controller.set_system_message(format!("[Steering rejected: {full}]"));
        }
        lp.batch.flush(lp.controller, true)?;
        return Ok(false);
    }
    let agent_key = agents::handle_agents_key(lp.controller, key, monitor)?;
    agents::apply_agent_key(lp.controller, lp.engine, &agent_key)?;
    if agent_key.consumed() || agents::try_open_agents(lp.controller, &key, lp.engine.agent_monitor())? {
        lp.batch.flush(lp.controller, true)?;
        return Ok(false);
    }
    let modal_res = handle_modal_key(lp.controller, key, &mut lp.batch.modal)?;
    match modal_res {
        ModalKeyResult::NotHandled => {
            if matches!(
                handle_autocomplete_key(lp.controller, res.completions, key),
                AutocompleteKeyResult::Handled
            ) {
                lp.batch.flush(lp.controller, true)?;
                return Ok(false);
            }
            dispatch_regular_key(lp, res, key).await
        }
        other => {
            apply_turn_modal_result(lp, other).await?;
            Ok(false)
        }
    }
}

async fn apply_turn_modal_result<B: TerminalBackend>(lp: &mut TurnLoop<'_, B>, res: ModalKeyResult) -> Result<()> {
    match res {
        ModalKeyResult::Handled | ModalKeyResult::NotHandled => {}
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            switch_modal_model(lp, &model, &provider, save_as_default).await?;
            return Ok(());
        }
        ModalKeyResult::ThinkingLevelSelected { level, save_as_default } => {
            lp.session.config.thinking_level = level.clone();
            save_or_print_thinking(lp.session, level.as_deref(), save_as_default).await;
            crate::repl::live::navigation::refresh_footer(lp.controller.state_mut(), lp.session, lp.engine);
        }
        ModalKeyResult::HelpCommandSelected { command } => {
            if !open_shared_help_modal(&command, lp.session, lp.engine, lp.controller)? {
                lp.controller
                    .set_system_message(format!("{command} is available after the current turn finishes"));
            }
        }
        action => {
            if !crate::repl::live::modal::btw::handle_btw_modal_action(lp.session, lp.engine, lp.controller, &action)
                .await
            {
                apply_shared_modal_action(lp.session, lp.engine, lp.controller, &action).await?;
            }
        }
    }
    lp.batch.flush(lp.controller, true)
}

async fn switch_modal_model<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    model: &str,
    provider: &str,
    save_as_default: bool,
) -> Result<()> {
    apply_turn_model_switch(TurnModelSwitchInput {
        route: rho_harness_core::provider::ModelRoute::new(provider, model),
        save_as_default,
        config: &mut lp.session.config,
        auth_store: &lp.session.auth_store,
        renderer: &lp.session.renderer,
        controller: lp.controller,
        batch: &mut lp.batch,
    })
    .await
}

async fn dispatch_regular_key<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    res: &mut TurnInputResources<'_>,
    key: KeyEvent,
) -> Result<bool> {
    let mut ctx = TurnInputContext {
        controller: lp.controller,
        history: res.history,
        completions: res.completions,
        batch: &mut lp.batch,
        steering: &lp.steering,
        session: lp.session,
        engine: Some(lp.engine),
    };
    if let TurnKeyResult::Cancelled = handle_turn_key(key, &mut ctx).await? {
        cancel_active_turn(lp, res.ui_events, res.cancellation).await?;
        return Ok(true);
    }
    Ok(false)
}
