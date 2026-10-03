//! Rig `Transport` driver for `AntigravityClient`.

use super::AntigravityClient;
use super::http::{PROVIDER_NAME, friendly_error};
use crate::adapter::rig::model::{AdapterFrame, AdapterWire};
use crate::antigravity::stream::unfold_antigravity_stream;
use crate::engine::compactor::llm::ModelHandle;
use rig::completion::CompletionRequest;
use rig::driver::{Exchange, Model, Opened, Opening, Transport};
use rig::error::ProviderError;

#[derive(Clone)]
pub struct AntigravityTransport {
    pub(crate) client: AntigravityClient,
}

impl Transport<AdapterWire> for AntigravityTransport {
    fn send(&self, payload: CompletionRequest, _exchange: Exchange) -> Opening<AdapterFrame> {
        let client = self.client.clone();
        Opening::new(async move {
            let response = client
                .open_stream(&payload)
                .await
                .map_err(|(status, body)| ProviderError::Provider(friendly_error(status, &body)))?;

            let stream = unfold_antigravity_stream(response);
            Ok(Opened::new(stream))
        })
    }
}

pub fn into_handle(client: AntigravityClient) -> ModelHandle {
    let wire = AdapterWire {
        name: PROVIDER_NAME.to_string(),
    };
    let transport = AntigravityTransport { client };
    Model::new(wire, transport).erase()
}
