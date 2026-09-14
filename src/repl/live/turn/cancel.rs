use super::footer::sync_turn_footer;
use super::input::reconcile_consumed_steering;
use super::runner::{TurnLoop, reset_controller_idle};
use crate::engine::runner::{CancellationSignal, TurnOutput};
use crate::error::Result;
use crate::repl::live::navigation::restore_queued_messages;
use crate::ui::interactive::{QueueKind, TerminalBackend, UiEvent};

pub(super) async fn cancel_active_turn<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    cancellation: &CancellationSignal,
) -> Result<()> {
    lp.batch.active_turn = false;
    cancellation.cancel();
    lp.steering.clear();
    reconcile_consumed_steering(lp.controller, &lp.steering);
    lp.controller
        .state_mut()
        .retain_queued(|msg| msg.kind != QueueKind::Steering);
    reset_controller_idle(lp.controller);
    lp.session.renderer.flush();
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    lp.engine.record_cancellation("operator interrupt").await?;
    restore_queued_messages(lp.controller);
    lp.session.renderer.print_notice("\nCanceled.\n");
    crate::platform::remote::set_active_steering(None);
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TurnEnd {
        stop_reason: "interrupted".to_string(),
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::StatusChanged {
        status: "idle".to_string(),
    });
    lp.batch.flush(lp.controller, false)
}

fn notify_turn_interruption<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    notice: &str,
) -> Result<()> {
    lp.batch.active_turn = false;
    reset_controller_idle(lp.controller);
    restore_queued_messages(lp.controller);
    lp.session.renderer.print_notice(notice);
    sync_turn_footer(lp.controller, lp.engine);
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    lp.batch.flush(lp.controller, false)
}

pub(super) fn finish_active_turn<B: TerminalBackend>(
    lp: &mut TurnLoop<'_, B>,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    result: Result<TurnOutput>,
) -> Result<()> {
    reconcile_consumed_steering(lp.controller, &lp.steering);
    lp.session.renderer.flush();
    lp.batch.active_turn = false;
    crate::platform::remote::set_active_steering(None);
    reset_controller_idle(lp.controller);
    sync_turn_footer(lp.controller, lp.engine);
    lp.batch.drain_events(lp.controller, ui_events)?;
    reset_controller_idle(lp.controller);
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::TurnEnd {
        stop_reason: "end_turn".to_string(),
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::StatusChanged {
        status: "idle".to_string(),
    });
    let totals = lp.engine.session_usage_totals();
    crate::platform::remote::PEER_REGISTRY.broadcast(&rho_harness_core::rpc::protocol::RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: lp.engine.context_percent_f64(),
        context_window: lp.engine.context_limit(),
        tokens_per_second: lp.engine.tokens_per_second(),
        quota: lp.engine.quota_display(),
    });
    lp.batch.flush(lp.controller, true)?;
    match result {
        Ok(out) if out.status == crate::engine::runner::RunStatus::Compacted => {
            notify_turn_interruption(
                lp,
                ui_events,
                "Context was compacted. Submit your prompt to proceed with compacted context.\n",
            )?;
        }
        Err(error) => {
            notify_turn_interruption(lp, ui_events, &format!("\nError: {error}\n"))?;
        }
        _ => {}
    }
    Ok(())
}
