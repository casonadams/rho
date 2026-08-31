use super::common::{capability_error, invalid_response, validate_provider_event};
use crate::process::PluginProcessClient;
use async_trait::async_trait;
use futures::stream::BoxStream;
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{
    AuthenticationRequest, AuthenticationResponse, ProviderCapability, ProviderDescriptor, ProviderRequest,
    ProviderStreamEvent,
};
use rho_sdk::protocol::{InvocationRequest, StreamEvent, TerminalResult};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ExternalProvider {
    pub(crate) client: PluginProcessClient,
    pub(crate) descriptor: ProviderDescriptor,
}

#[async_trait]
impl ProviderCapability for ExternalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    async fn authenticate(&self, request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        let output = self
            .client
            .invoke(
                self.descriptor.id.clone(),
                InvocationRequest::ProviderAuthenticate(request),
            )
            .await
            .map_err(capability_error)?;
        if !output.events.is_empty() {
            return Err(invalid_response());
        }
        match output.terminal {
            TerminalResult::ProviderAuthenticated(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        let running = self
            .client
            .start_invocation(self.descriptor.id.clone(), InvocationRequest::ProviderStream(request))
            .await
            .map_err(capability_error)?;
        let (sender, receiver) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut running = running;
            loop {
                let event = tokio::select! {
                    _ = sender.closed() => {
                        let _ = running.cancel().await;
                        return;
                    }
                    event = running.next_event() => event,
                };
                let Some(event) = event else {
                    break;
                };
                let StreamEvent::Provider(event) = event else {
                    let _ = sender.send(Err(invalid_response())).await;
                    let _ = running.cancel().await;
                    return;
                };
                if validate_provider_event(&event).is_err() {
                    let _ = sender.send(Err(invalid_response())).await;
                    let _ = running.cancel().await;
                    return;
                }
                if sender.send(Ok(event)).await.is_err() {
                    let _ = running.cancel().await;
                    return;
                }
            }
            match running.finish().await {
                Ok(TerminalResult::StreamCompleted) => {}
                Ok(_) => {
                    let _ = sender.send(Err(invalid_response())).await;
                }
                Err(error) => {
                    let _ = sender.send(Err(capability_error(error))).await;
                }
            }
        });
        Ok(Box::pin(futures::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        })))
    }
}
