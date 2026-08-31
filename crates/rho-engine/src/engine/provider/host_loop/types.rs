use rho_sdk::contract::{FinishReason, ModelMessage, ProviderToolDefinition, ScopedCredential};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub last_input_tokens: u64,
    #[serde(default)]
    pub generation_elapsed_ms: u64,
}

impl ProviderUsage {
    pub fn add(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
        if input_tokens > 0 {
            self.last_input_tokens = input_tokens;
        }
    }

    pub fn add_duration(&mut self, elapsed_ms: u64) {
        self.generation_elapsed_ms = self.generation_elapsed_ms.saturating_add(elapsed_ms);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationCheckpoint {
    pub messages: Vec<ModelMessage>,
    pub completed_model_turns: usize,
    pub usage: ProviderUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralTurnRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub credential: Option<ScopedCredential>,
    pub max_output_tokens: Option<u64>,
    pub tools: Vec<ProviderToolDefinition>,
    pub max_turns: usize,
    pub checkpoint: Option<ContinuationCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralTurnOutput {
    pub text: String,
    pub messages: Vec<ModelMessage>,
    pub usage: ProviderUsage,
    pub finish_reason: FinishReason,
    pub model_turns: usize,
    pub tool_calls: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeutralTurnTerminal {
    Completed(NeutralTurnOutput),
    Checkpoint(ContinuationCheckpoint),
    Cancelled(ContinuationCheckpoint),
}
