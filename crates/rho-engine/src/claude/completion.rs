//! Rig `Transport` driver for `ClaudeClient`.

use super::ClaudeClient;
use super::http::{PROVIDER_NAME, friendly_error};
use super::stream::unfold_claude_stream;
use crate::adapter::rig::model::{AdapterFrame, AdapterWire};
use crate::engine::compactor::llm::ModelHandle;
use rig::completion::CompletionRequest;
use rig::driver::{Exchange, Model, Opened, Opening, Transport};
use rig::error::ProviderError;

#[derive(Clone)]
pub struct ClaudeTransport {
    pub(crate) client: ClaudeClient,
}

impl Transport<AdapterWire> for ClaudeTransport {
    fn send(&self, payload: CompletionRequest, _exchange: Exchange) -> Opening<AdapterFrame> {
        let client = self.client.clone();
        Opening::new(async move {
            let response = client
                .open_stream(&payload)
                .await
                .map_err(|(status, body)| ProviderError::Provider(friendly_error(status, &body)))?;

            let stream = unfold_claude_stream(response);
            Ok(Opened::new(stream))
        })
    }
}

pub fn into_handle(client: ClaudeClient) -> ModelHandle {
    let wire = AdapterWire {
        name: PROVIDER_NAME.to_string(),
    };
    let transport = ClaudeTransport { client };
    Model::new(wire, transport).erase()
}
