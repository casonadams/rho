use async_trait::async_trait;

use crate::engine::AgentEngine;
use crate::engine::runner::{CancellationSignal, TurnRequest};
use crate::error::AppError;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::QueuedMessage;

use super::types::{ActivePromptRunner, SharedSteeringQueue};

pub struct ReplAgentRunner<'a> {
    engine: &'a AgentEngine,
    renderer: &'a TerminalRenderer,
    cancellation: CancellationSignal,
    steering: SharedSteeringQueue,
}

impl<'a> ReplAgentRunner<'a> {
    pub fn new(engine: &'a AgentEngine, renderer: &'a TerminalRenderer) -> Self {
        Self {
            engine,
            renderer,
            cancellation: CancellationSignal::default(),
            steering: SharedSteeringQueue::new(engine.config.steering_mode),
        }
    }
}

#[async_trait]
impl ActivePromptRunner for ReplAgentRunner<'_> {
    type Error = AppError;

    async fn run_prompt(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.engine
            .run_turn(
                TurnRequest {
                    prompt: &prompt.text,
                    cancellation: Some(&self.cancellation),
                    steering: Some(&self.steering),
                },
                std::sync::Arc::new(self.renderer.clone()),
            )
            .await
            .map(|_| ())
    }

    async fn steer(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.steering.enqueue(prompt.text.clone());
        self.cancellation.interrupt_stream();
        Ok(())
    }

    async fn cancel_active(&self) -> Result<(), Self::Error> {
        self.cancellation.cancel();
        self.engine.record_cancellation("operator interrupt").await
    }
}
