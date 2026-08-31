use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{PermissionCapability, PermissionDecision, RequestedOperation};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

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

#[cfg(test)]
mod tests {
    use super::*;
    use rho_sdk::contract::InvocationContext;

    fn operation(tool_name: &str) -> RequestedOperation {
        RequestedOperation {
            tool_id: format!("tool:{tool_name}").parse().unwrap(),
            arguments: serde_json::json!({}),
            effects: Vec::new(),
            context: InvocationContext::new("session", ".", false),
        }
    }

    struct Static(PermissionDecision);

    #[async_trait::async_trait]
    impl PermissionCapability for Static {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "permission:fixture".parse().unwrap()
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            Ok(self.0.clone())
        }
    }

    struct Failing;

    #[async_trait::async_trait]
    impl PermissionCapability for Failing {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "permission:fixture".parse().unwrap()
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            Err(CapabilityError::Unavailable {
                message: "fixture failure".to_string(),
            })
        }
    }

    struct Panicking;

    #[async_trait::async_trait]
    impl PermissionCapability for Panicking {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "permission:fixture".parse().unwrap()
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            panic!("policy panicked");
        }
    }

    struct Sleeping;

    #[async_trait::async_trait]
    impl PermissionCapability for Sleeping {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "permission:fixture".parse().unwrap()
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(PermissionDecision::Allow)
        }
    }

    struct Invalid;

    #[async_trait::async_trait]
    impl PermissionCapability for Invalid {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "permission:fixture".parse().unwrap()
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            Ok(PermissionDecision::Deny {
                rationale: "   ".to_string(),
            })
        }
    }

    fn request(default: PermissionDecision) -> PermissionRequest {
        PermissionRequest {
            operation: operation("read"),
            default_decision: default,
        }
    }

    fn evaluator(policies: Vec<Arc<dyn PermissionCapability>>) -> PolicyEvaluator {
        PolicyEvaluator::spawn(policies, PolicyFailureMode::Deny, PolicyLimits::default())
    }

    #[tokio::test]
    async fn composition_selects_the_most_restrictive_valid_outcome() {
        let cases: [(Vec<PermissionDecision>, PermissionDecision, PermissionDecision); 3] = [
            (
                vec![
                    PermissionDecision::Allow,
                    PermissionDecision::Deny { rationale: "no".into() },
                ],
                PermissionDecision::Allow,
                PermissionDecision::Deny { rationale: "no".into() },
            ),
            (
                vec![
                    PermissionDecision::Allow,
                    PermissionDecision::ApprovalRequired {
                        rationale: "why".into(),
                    },
                ],
                PermissionDecision::Allow,
                PermissionDecision::ApprovalRequired {
                    rationale: "why".into(),
                },
            ),
            (
                vec![PermissionDecision::Allow],
                PermissionDecision::Deny {
                    rationale: "floor".into(),
                },
                PermissionDecision::Deny {
                    rationale: "floor".into(),
                },
            ),
        ];
        for (decisions, default, expected) in cases {
            let policies = decisions
                .into_iter()
                .map(|d| Arc::new(Static(d)) as Arc<dyn PermissionCapability>)
                .collect();
            let outcome = evaluator(policies).evaluate(request(default)).await.unwrap();
            assert_eq!(outcome, expected);
        }
    }

    #[tokio::test]
    async fn a_failed_policy_denies_only_operations_that_required_approval() {
        let evaluator = evaluator(vec![Arc::new(Failing)]);
        let denied = evaluator
            .evaluate(request(PermissionDecision::ApprovalRequired {
                rationale: "ask".into(),
            }))
            .await
            .unwrap();
        assert_eq!(
            denied,
            PermissionDecision::Deny {
                rationale: POLICY_UNAVAILABLE_DENIAL.to_string()
            }
        );
        let allowed = evaluator.evaluate(request(PermissionDecision::Allow)).await.unwrap();
        assert_eq!(allowed, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn failures_surface_as_errors_in_surface_mode() {
        let evaluator = PolicyEvaluator::spawn(
            vec![Arc::new(Failing)],
            PolicyFailureMode::Surface,
            PolicyLimits::default(),
        );
        let outcome = evaluator.evaluate(request(PermissionDecision::Allow)).await;
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn panicking_policies_deny_approval_evaluated_operations() {
        let evaluator = evaluator(vec![Arc::new(Panicking)]);
        let denied = evaluator
            .evaluate(request(PermissionDecision::ApprovalRequired {
                rationale: "ask".into(),
            }))
            .await
            .unwrap();
        assert!(matches!(denied, PermissionDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn timed_out_policies_deny_approval_evaluated_operations() {
        let evaluator = PolicyEvaluator::spawn(
            vec![Arc::new(Sleeping)],
            PolicyFailureMode::Deny,
            PolicyLimits {
                evaluation_timeout: Duration::from_millis(20),
                ..PolicyLimits::default()
            },
        );
        let denied = evaluator
            .evaluate(request(PermissionDecision::ApprovalRequired {
                rationale: "ask".into(),
            }))
            .await
            .unwrap();
        assert!(matches!(denied, PermissionDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn invalid_decisions_fail_closed_for_approval_evaluated_operations() {
        let deny_mode = evaluator(vec![Arc::new(Invalid)]);
        let denied = deny_mode
            .evaluate(request(PermissionDecision::ApprovalRequired {
                rationale: "ask".into(),
            }))
            .await
            .unwrap();
        assert!(matches!(denied, PermissionDecision::Deny { .. }));

        let surface = PolicyEvaluator::spawn(
            vec![Arc::new(Invalid)],
            PolicyFailureMode::Surface,
            PolicyLimits::default(),
        );
        assert!(surface.evaluate(request(PermissionDecision::Allow)).await.is_err());
    }

    #[test]
    fn constructing_without_policies_requires_no_runtime() {
        let evaluator = PolicyEvaluator::spawn(Vec::new(), PolicyFailureMode::Deny, PolicyLimits::default());
        assert!(evaluator.commands.is_none());
    }
}
