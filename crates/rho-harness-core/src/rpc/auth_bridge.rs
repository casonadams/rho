use crate::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};
use crate::error::{AppError, Result};
use crate::locking::lock;
use crate::rpc::protocol::RpcEvent;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Default)]
pub struct AuthInputResponse {
    pub secret_value: Option<String>,
    pub selected_option: Option<String>,
}

type PendingInputs = Mutex<HashMap<String, oneshot::Sender<AuthInputResponse>>>;

#[derive(Clone, Default)]
pub struct RpcAuthBridge {
    pending_inputs: Arc<PendingInputs>,
}

impl RpcAuthBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve_input(&self, interaction_id: &str, response: AuthInputResponse) -> bool {
        let pending = lock(&self.pending_inputs).remove(interaction_id);
        pending.is_some_and(|tx| tx.send(response).is_ok())
    }

    pub fn callbacks(
        &self,
        provider: impl Into<String>,
        event_tx: mpsc::UnboundedSender<RpcEvent>,
    ) -> RpcOAuthCallbacks {
        RpcOAuthCallbacks {
            provider: provider.into(),
            event_tx,
            pending_inputs: Arc::clone(&self.pending_inputs),
        }
    }
}

pub struct RpcOAuthCallbacks {
    provider: String,
    event_tx: mpsc::UnboundedSender<RpcEvent>,
    pending_inputs: Arc<PendingInputs>,
}

impl RpcOAuthCallbacks {
    fn send(&self, event: RpcEvent) -> Result<()> {
        self.event_tx
            .send(event)
            .map_err(|_| AppError::Auth(format!("{} login: the client is no longer listening", self.provider)))
    }

    /// Registers a reply slot before the request is sent, so a fast reply cannot be missed.
    async fn ask(&self, request: impl FnOnce(String) -> RpcEvent, cancelled: &str) -> Result<AuthInputResponse> {
        let interaction_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        lock(&self.pending_inputs).insert(interaction_id.clone(), tx);
        if let Err(error) = self.send(request(interaction_id.clone())) {
            lock(&self.pending_inputs).remove(&interaction_id);
            return Err(error);
        }
        rx.await.map_err(|_| AppError::Auth(cancelled.to_string()))
    }
}

#[async_trait]
impl OAuthLoginCallbacks for RpcOAuthCallbacks {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()> {
        self.send(RpcEvent::AuthRequest {
            interaction_id: uuid::Uuid::new_v4().to_string(),
            provider: self.provider.clone(),
            auth_url: Some(url.to_string()),
            instructions: instructions.map(ToString::to_string),
            user_code: None,
            prompt: None,
            is_secret: None,
            options: None,
        })
    }

    async fn on_device_code(&self, info: &DeviceCodeInfo<'_>) -> Result<()> {
        self.send(RpcEvent::AuthRequest {
            interaction_id: uuid::Uuid::new_v4().to_string(),
            provider: self.provider.clone(),
            auth_url: Some(info.verification_uri.to_string()),
            instructions: None,
            user_code: Some(info.user_code.to_string()),
            prompt: None,
            is_secret: None,
            options: None,
        })
    }

    async fn on_prompt(&self, message: &str, secret: bool) -> Result<String> {
        let response = self
            .ask(
                |interaction_id| RpcEvent::AuthRequest {
                    interaction_id,
                    provider: self.provider.clone(),
                    auth_url: None,
                    instructions: None,
                    user_code: None,
                    prompt: Some(message.to_string()),
                    is_secret: Some(secret),
                    options: None,
                },
                "auth prompt cancelled or disconnected",
            )
            .await?;
        Ok(response.secret_value.unwrap_or_default())
    }

    async fn on_select(&self, message: &str, options: &[SelectOption]) -> Result<Option<String>> {
        let response = self
            .ask(
                |interaction_id| RpcEvent::AuthRequest {
                    interaction_id,
                    provider: self.provider.clone(),
                    auth_url: None,
                    instructions: None,
                    user_code: None,
                    prompt: Some(message.to_string()),
                    is_secret: None,
                    options: Some(options.to_vec()),
                },
                "auth selection cancelled or disconnected",
            )
            .await?;
        Ok(response.selected_option)
    }

    async fn on_progress(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_rpc_auth_bridge_prompt_roundtrip() {
        let bridge = RpcAuthBridge::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let callbacks = bridge.callbacks("test-prov", event_tx);

        let bridge_clone = bridge.clone();
        let prompt_task = tokio::spawn(async move { callbacks.on_prompt("Enter token", true).await });

        let event = event_rx.recv().await.unwrap();
        let id = match event {
            RpcEvent::AuthRequest {
                interaction_id,
                prompt,
                is_secret,
                ..
            } => {
                assert_eq!(prompt, Some("Enter token".to_string()));
                assert_eq!(is_secret, Some(true));
                interaction_id
            }
            other => panic!("expected AuthRequest, got {other:?}"),
        };

        let resolved = bridge_clone.resolve_input(
            &id,
            AuthInputResponse {
                secret_value: Some("super-secret-token".to_string()),
                selected_option: None,
            },
        );
        assert!(resolved);

        let result = prompt_task.await.unwrap().unwrap();
        assert_eq!(result, "super-secret-token");
    }

    #[tokio::test]
    async fn test_rpc_auth_bridge_select_roundtrip() {
        let bridge = RpcAuthBridge::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let callbacks = bridge.callbacks("test-prov", event_tx);

        let bridge_clone = bridge.clone();
        let select_task = tokio::spawn(async move {
            callbacks
                .on_select("Pick option", &[SelectOption::new("opt1", "Option 1")])
                .await
        });

        let event = event_rx.recv().await.unwrap();
        let id = match event {
            RpcEvent::AuthRequest {
                interaction_id,
                options,
                ..
            } => {
                assert_eq!(options.unwrap().len(), 1);
                interaction_id
            }
            other => panic!("expected AuthRequest, got {other:?}"),
        };

        bridge_clone.resolve_input(
            &id,
            AuthInputResponse {
                secret_value: None,
                selected_option: Some("opt1".to_string()),
            },
        );

        let result = select_task.await.unwrap().unwrap();
        assert_eq!(result, Some("opt1".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn prompt_with_a_dropped_client_fails_and_forgets_the_interaction() {
        let bridge = RpcAuthBridge::new();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        drop(event_rx);
        let callbacks = bridge.callbacks("test-prov", event_tx);

        let result = tokio::time::timeout(Duration::from_secs(1), callbacks.on_prompt("Enter token", true))
            .await
            .expect("prompt must not wait for a reply nobody can send");

        assert!(matches!(result, Err(AppError::Auth(_))));
        assert!(bridge.pending_inputs.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn select_with_a_dropped_client_fails_instead_of_hanging() {
        let bridge = RpcAuthBridge::new();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        drop(event_rx);
        let callbacks = bridge.callbacks("test-prov", event_tx);

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            callbacks.on_select("Pick option", &[SelectOption::new("opt1", "Option 1")]),
        )
        .await
        .expect("selection must not wait for a reply nobody can send");

        assert!(matches!(result, Err(AppError::Auth(_))));
        assert!(bridge.pending_inputs.lock().unwrap().is_empty());
    }
}
