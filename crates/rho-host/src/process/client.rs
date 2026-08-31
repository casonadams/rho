use super::errors::ProcessError;
use super::framing;
use super::session::{InvocationSession, SupervisedProcess, new_request_id};
use rho_sdk::capability::ValidatedManifest;
use rho_sdk::contract::CapabilityDescriptor;
use rho_sdk::protocol::{Envelope, InvocationRequest, ProtocolMessage, StreamEvent, TerminalResult};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy)]
pub struct ProcessLimits {
    pub startup_timeout: Duration,
    pub discovery_timeout: Duration,
    pub invocation_timeout: Duration,
    pub cancellation_timeout: Duration,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(5),
            discovery_timeout: Duration::from_secs(5),
            invocation_timeout: Duration::from_secs(60),
            cancellation_timeout: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginProcessClient {
    executable: Arc<PathBuf>,
    limits: ProcessLimits,
}

impl PluginProcessClient {
    pub fn new(executable: impl Into<PathBuf>, limits: ProcessLimits) -> Self {
        Self {
            executable: Arc::new(executable.into()),
            limits,
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub async fn discover(&self) -> Result<ProcessDiscovery, ProcessError> {
        let mut process = self.start().await?;
        let request_id = new_request_id("discovery")?;
        let request = Envelope::new(request_id.clone(), ProtocolMessage::DiscoveryRequest);
        if let Err(error) = process.write(&request).await {
            return Err(process.fail(error).await);
        }
        let response = match tokio::time::timeout(
            self.limits.discovery_timeout,
            framing::read_envelope(&mut process.output),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(process.fail(error).await),
            Err(_) => return Err(process.fail(ProcessError::DiscoveryTimeout).await),
        };
        if response.request_id != request_id {
            return Err(process.fail(ProcessError::CorrelationMismatch).await);
        }
        let ProtocolMessage::TerminalResponse {
            result: TerminalResult::Discovery { manifest, capabilities },
        } = response.message
        else {
            return Err(process.fail(ProcessError::UnexpectedResponse).await);
        };
        let manifest = match manifest.validate() {
            Ok(manifest) => manifest,
            Err(_) => return Err(process.fail(ProcessError::InvalidManifest).await),
        };
        let stderr_redacted = process.stop().await;
        Ok(ProcessDiscovery {
            manifest,
            capabilities,
            stderr_redacted,
        })
    }

    pub async fn start_invocation(
        &self,
        capability_id: rho_sdk::capability::CapabilityId,
        invocation: InvocationRequest,
    ) -> Result<RunningInvocation, ProcessError> {
        let mut process = self.start().await?;
        let request_id = new_request_id("invoke")?;
        let request = Envelope::new(
            request_id.clone(),
            ProtocolMessage::InvocationRequest {
                capability_id,
                invocation,
            },
        );
        if let Err(error) = process.write(&request).await {
            return Err(process.fail(error).await);
        }

        let (event_sender, event_receiver) = mpsc::channel(32);
        let (completion_sender, completion_receiver) = oneshot::channel();
        let (cancel_sender, cancel_receiver) = oneshot::channel();
        let limits = self.limits;
        let task = tokio::spawn(async move {
            let mut invocation = InvocationSession {
                process: &mut process,
                request_id,
                events: event_sender,
                limits,
            };
            let result = tokio::time::timeout(limits.invocation_timeout, invocation.run(cancel_receiver))
                .await
                .unwrap_or(Err(ProcessError::InvocationTimeout));
            let result = match result {
                Ok(terminal) => {
                    process.stop().await;
                    Ok(terminal)
                }
                Err(error) => Err(process.fail(error).await),
            };
            let _ = completion_sender.send(result);
        });
        Ok(RunningInvocation {
            events: event_receiver,
            completion: Some(completion_receiver),
            cancel: Some(cancel_sender),
            task,
        })
    }

    pub async fn invoke(
        &self,
        capability_id: rho_sdk::capability::CapabilityId,
        invocation: InvocationRequest,
    ) -> Result<InvocationOutput, ProcessError> {
        let mut running = self.start_invocation(capability_id, invocation).await?;
        let mut events = Vec::new();
        while let Some(event) = running.next_event().await {
            events.push(event);
        }
        let terminal = running.finish().await?;
        Ok(InvocationOutput { events, terminal })
    }

    async fn start(&self) -> Result<SupervisedProcess, ProcessError> {
        let mut process = SupervisedProcess::spawn(&self.executable).await?;
        let handshake = tokio::time::timeout(self.limits.startup_timeout, process.handshake()).await;
        match handshake {
            Ok(Ok(())) => Ok(process),
            Ok(Err(error)) => Err(process.fail(error).await),
            Err(_) => Err(process.fail(ProcessError::StartupTimeout).await),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDiscovery {
    pub manifest: ValidatedManifest,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub stderr_redacted: bool,
}

impl ProcessDiscovery {
    pub fn validate_strict(&self) -> Result<(), ProcessError> {
        let declared: BTreeSet<_> = self
            .manifest
            .capabilities
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect();
        let mut described = BTreeSet::new();
        for descriptor in &self.capabilities {
            descriptor.validate().map_err(|_| ProcessError::InvalidCapability)?;
            if !described.insert(descriptor.id().clone()) {
                return Err(ProcessError::InvalidCapability);
            }
        }
        if declared == described {
            Ok(())
        } else {
            Err(ProcessError::InvalidCapability)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationOutput {
    pub events: Vec<StreamEvent>,
    pub terminal: TerminalResult,
}

pub struct RunningInvocation {
    pub(crate) events: mpsc::Receiver<StreamEvent>,
    pub(crate) completion: Option<oneshot::Receiver<Result<TerminalResult, ProcessError>>>,
    pub(crate) cancel: Option<oneshot::Sender<()>>,
    pub(crate) task: tokio::task::JoinHandle<()>,
}

impl RunningInvocation {
    pub async fn next_event(&mut self) -> Option<StreamEvent> {
        self.events.recv().await
    }

    pub async fn finish(mut self) -> Result<TerminalResult, ProcessError> {
        self.cancel.take();
        let completion = self.completion.take().ok_or(ProcessError::ProcessTaskFailed)?;
        completion.await.map_err(|_| ProcessError::ProcessTaskFailed)?
    }

    pub async fn cancel(mut self) -> Result<TerminalResult, ProcessError> {
        self.events.close();
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        let completion = self.completion.take().ok_or(ProcessError::ProcessTaskFailed)?;
        completion.await.map_err(|_| ProcessError::ProcessTaskFailed)?
    }
}

impl Drop for RunningInvocation {
    fn drop(&mut self) {
        self.events.close();
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if self.completion.is_none() {
            self.task.abort();
        }
    }
}
