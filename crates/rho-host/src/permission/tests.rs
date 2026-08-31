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
