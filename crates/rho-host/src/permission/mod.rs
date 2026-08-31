use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{PermissionCapability, PermissionDecision, RequestedOperation};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
mod tests;

const POLICY_UNAVAILABLE_DENIAL: &str = "permission policy unavailable; denied by default";
const PANICKED_POLICY_MESSAGE: &str = "permission policy evaluation panicked";
const TIMED_OUT_POLICY_MESSAGE: &str = "permission policy evaluation timed out";
const INVALID_DECISION_MESSAGE: &str = "permission policy returned an invalid decision";
const UNAVAILABLE_EVALUATOR_MESSAGE: &str = "permission policy evaluator is unavailable";

/// Configured behavior when a policy fails, times out, or is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolicyFailureMode {
    /// A failed policy denies operations that require permission evaluation.
    #[default]
    Deny,
    /// The policy failure is surfaced as an error for the operation.
    Surface,
}

/// Bounded coordination limits for policy evaluation.
#[derive(Debug, Clone, Copy)]
pub struct PolicyLimits {
    pub evaluation_timeout: Duration,
    pub mailbox: usize,
}

impl Default for PolicyLimits {
    fn default() -> Self {
        Self {
            evaluation_timeout: Duration::from_secs(30),
            mailbox: 8,
        }
    }
}

/// A permission evaluation: the normalized operation plus the built-in default
/// policy's decision, which participates in restrictive composition.
pub struct PermissionRequest {
    pub operation: RequestedOperation,
    pub default_decision: PermissionDecision,
}

struct PolicyCommand {
    request: PermissionRequest,
    reply: oneshot::Sender<Result<PermissionDecision, CapabilityError>>,
}

/// Evaluates all applicable active permission policies and selects the most
/// restrictive valid outcome.
///
/// Lightweight actor semantics on Tokio only: a bounded command mailbox, one
/// evaluation at a time, typed oneshot replies, per-policy deadlines, and panic
/// containment. No actor framework. With no configured policies the default
/// decision passes through without spawning or channel round-trips.
#[derive(Clone)]
pub struct PolicyEvaluator {
    commands: Option<mpsc::Sender<PolicyCommand>>,
}

impl PolicyEvaluator {
    pub fn spawn(
        policies: Vec<Arc<dyn PermissionCapability>>,
        failure: PolicyFailureMode,
        limits: PolicyLimits,
    ) -> Self {
        if policies.is_empty() {
            return Self { commands: None };
        }
        let (commands, mailbox) = mpsc::channel(limits.mailbox.max(1));
        tokio::spawn(actor_loop(
            mailbox,
            policies,
            PolicyRuntime {
                failure,
                timeout: limits.evaluation_timeout,
            },
        ));
        Self {
            commands: Some(commands),
        }
    }

    pub async fn evaluate(&self, request: PermissionRequest) -> Result<PermissionDecision, CapabilityError> {
        let Some(commands) = &self.commands else {
            return Ok(request.default_decision);
        };
        let (reply, response) = oneshot::channel();
        commands
            .send(PolicyCommand { request, reply })
            .await
            .map_err(|_| unavailable_evaluator())?;
        response.await.map_err(|_| unavailable_evaluator())?
    }
}

/// Coordination configuration per evaluator actor.
struct PolicyRuntime {
    failure: PolicyFailureMode,
    timeout: Duration,
}

async fn actor_loop(
    mut mailbox: mpsc::Receiver<PolicyCommand>,
    policies: Vec<Arc<dyn PermissionCapability>>,
    runtime: PolicyRuntime,
) {
    while let Some(command) = mailbox.recv().await {
        let decision = compose(&policies, command.request, &runtime).await;
        let _ = command.reply.send(decision);
    }
}

async fn compose(
    policies: &[Arc<dyn PermissionCapability>],
    request: PermissionRequest,
    runtime: &PolicyRuntime,
) -> Result<PermissionDecision, CapabilityError> {
    let mut outcome = request.default_decision;
    let mut breakdown = None;
    for policy in policies {
        match evaluate_policy(Arc::clone(policy), request.operation.clone(), runtime.timeout).await {
            Ok(decision) => outcome = restrict(outcome, decision),
            Err(error) => {
                breakdown.get_or_insert(error);
            }
        }
    }
    match breakdown {
        Some(error) => match runtime.failure {
            PolicyFailureMode::Surface => Err(error),
            PolicyFailureMode::Deny if requires_approval(&outcome) => Ok(PermissionDecision::Deny {
                rationale: POLICY_UNAVAILABLE_DENIAL.to_string(),
            }),
            PolicyFailureMode::Deny => Ok(outcome),
        },
        None => Ok(outcome),
    }
}

async fn evaluate_policy(
    policy: Arc<dyn PermissionCapability>,
    operation: RequestedOperation,
    timeout: Duration,
) -> Result<PermissionDecision, CapabilityError> {
    let mut evaluation = tokio::spawn(async move { policy.evaluate(operation).await });
    match tokio::time::timeout(timeout, &mut evaluation).await {
        Ok(Ok(Ok(decision))) => decision.validate().map(|()| decision).map_err(|_| invalid_decision()),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => Err(CapabilityError::Unavailable {
            message: PANICKED_POLICY_MESSAGE.to_string(),
        }),
        Err(_) => {
            evaluation.abort();
            Err(CapabilityError::Unavailable {
                message: TIMED_OUT_POLICY_MESSAGE.to_string(),
            })
        }
    }
}

fn restrict(current: PermissionDecision, incoming: PermissionDecision) -> PermissionDecision {
    if restrictiveness(&incoming) > restrictiveness(&current) {
        incoming
    } else {
        current
    }
}

fn restrictiveness(decision: &PermissionDecision) -> u8 {
    match decision {
        PermissionDecision::Allow => 0,
        PermissionDecision::ApprovalRequired { .. } => 1,
        PermissionDecision::Deny { .. } => 2,
    }
}

fn requires_approval(decision: &PermissionDecision) -> bool {
    !matches!(decision, PermissionDecision::Allow)
}

fn unavailable_evaluator() -> CapabilityError {
    CapabilityError::Unavailable {
        message: UNAVAILABLE_EVALUATOR_MESSAGE.to_string(),
    }
}

fn invalid_decision() -> CapabilityError {
    CapabilityError::Unavailable {
        message: INVALID_DECISION_MESSAGE.to_string(),
    }
}
