use crossterm::event::{Event, KeyEvent, KeyEventKind};

use super::cancel::cancel_active_turn;
use super::input::{TurnInputContext, TurnKeyResult, handle_turn_key};
use super::model_switch::{TurnModelSwitchInput, apply_turn_model_switch};
use super::runner::TurnLoop;
use crate::engine::runner::CancellationSignal;
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, handle_modal_paste};
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

fn handle_paste<B: TerminalBackend>(lp: &mut TurnLoop<'_, B>, text: String) -> Result<()> {
    if !handle_modal_paste(lp.controller, &text) {
        lp.controller.state_mut().apply(UiAction::Paste(text));
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
            handle_paste(lp, text)?;
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
    let modal_res = handle_modal_key(lp.controller, key, &mut lp.batch.modal)?;
    match modal_res {
        ModalKeyResult::NotHandled => dispatch_regular_key(lp, res, key).await,
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            switch_modal_model(lp, (&model, &provider), save_as_default).await?;
            Ok(false)
        }
        _ => Ok(false),
    }
}

async fn switch_modal_model<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    (model, provider): (&str, &str),
    save_as_default: bool,
) -> Result<()> {
    apply_turn_model_switch(TurnModelSwitchInput {
        model,
        provider,
        save_as_default,
        config: &mut lp.session.config,
        auth_store: &lp.session.auth_store,
        renderer: &lp.session.renderer,
        controller: lp.controller,
        model_switch: &lp.model_switch,
        batch: &mut lp.batch,
        shared_auth: Some(lp.engine.shared_auth_store()),
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
        model_switch: &lp.model_switch,
        shared_auth: Some(lp.engine.shared_auth_store()),
    };
    if let TurnKeyResult::Cancelled = handle_turn_key(key, &mut ctx).await? {
        cancel_active_turn(lp, res.ui_events, res.cancellation).await?;
        return Ok(true);
    }
    Ok(false)
}
