use std::sync::Arc;

use super::footer::sync_turn_footer;
use super::input::reconcile_consumed_steering;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::repl::live::batch::{LiveBatch, SPINNER_FRAME_INTERVALS};
use crate::ui::interactive::{Activity, TerminalBackend, TerminalController, UiEvent};

pub(super) fn reset_controller_idle<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    controller.clear_active_tool();
    controller.state_mut().footer_mut().activity = Activity::Idle;
    controller.state_mut().footer_mut().running_tool = None;
}

pub(super) struct TurnLoop<'a, B: TerminalBackend> {
    pub session: &'a mut crate::repl::ReplSession,
    pub engine: &'a AgentEngine,
    pub controller: &'a mut TerminalController<B>,
    pub steering: Arc<SharedSteeringQueue>,
    pub model_switch: Arc<rho_engine::engine::runner::SharedModelSwitch>,
    pub batch: LiveBatch,
    pub spinner_tick: usize,
}

impl<'a, B: TerminalBackend> TurnLoop<'a, B> {
    pub fn new(
        (session, engine): (&'a mut crate::repl::ReplSession, &'a AgentEngine),
        controller: &'a mut TerminalController<B>,
        (steering, model_switch): (
            Arc<SharedSteeringQueue>,
            Arc<rho_engine::engine::runner::SharedModelSwitch>,
        ),
    ) -> Self {
        controller.state_mut().footer_mut().activity = Activity::Working;
        sync_turn_footer(controller, engine);
        Self {
            session,
            engine,
            controller,
            steering,
            model_switch,
            batch: LiveBatch::turn(),
            spinner_tick: 0,
        }
    }

    pub fn on_tick(&mut self) -> Result<()> {
        let steering = reconcile_consumed_steering(self.controller, &self.steering);
        self.spinner_tick += 1;
        let spinner = self.tick_spinner();
        let expired = self.controller.check_system_message_expiration();
        let footer = sync_turn_footer(self.controller, self.engine);
        self.batch
            .flush(self.controller, spinner || footer || expired || steering)?;
        if matches!(self.controller.state().footer().activity, Activity::Idle) {
            self.controller.state_mut().footer_mut().activity = Activity::Working;
        }
        Ok(())
    }

    fn tick_spinner(&mut self) -> bool {
        if self.spinner_tick < SPINNER_FRAME_INTERVALS {
            return false;
        }
        self.spinner_tick = 0;
        self.controller.advance_spinner();
        !matches!(self.controller.state().footer().activity, Activity::Idle)
    }

    pub fn drain_ui_batch(&mut self, ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Result<()> {
        let steering = reconcile_consumed_steering(self.controller, &self.steering);
        let mut dirty = false;
        while let Ok(next) = ui_events.try_recv() {
            dirty |= self.batch.push_event(self.controller, next)?;
        }
        let footer = sync_turn_footer(self.controller, self.engine);
        if dirty || footer || steering {
            self.batch.flush(self.controller, footer || steering)?;
        }
        if matches!(self.controller.state().footer().activity, Activity::Idle) {
            self.controller.state_mut().footer_mut().activity = Activity::Working;
        }
        Ok(())
    }
}
