//! Three-tier P1/P2/P3 guard evaluation engine with `.guard.yml` config,
//! production/preview environment awareness, and audit log integration.

use super::config::{
    GuardConfig, ResolvedGuardThresholds, is_allowlisted_command, is_command_targeting_preview, load_guard_config,
    resolve_preview_markers, resolve_production_markers, resolve_thresholds,
};
use super::danger::check_critical_danger;
use super::parser::{GuardVerdict, parse_guard_output};
use super::prompt::GUARD_SYSTEM_PROMPT;
use super::telemetry::{
    AuditRecordParams, DecisionSource, DecisionStateRecord, DecisionTargetKey, ModelVerdictRecord,
    append_decision_record, build_decision_record, is_telemetry_enabled, resolve_telemetry_path,
};
use crate::decision::{DecisionHandle, DecisionRequest, DecisionResponse};
use crate::engine::compactor::llm::ModelHandle;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone)]
enum GuardBackend {
    Chat(ModelHandle),
    Decision(DecisionHandle),
}

struct EvalContext {
    state: DecisionStateRecord,
    started: Instant,
    config: GuardConfig,
}

#[derive(Clone)]
pub struct GuardEvaluator {
    backend: GuardBackend,
    config_override: Option<GuardConfig>,
    telemetry_path_override: Option<PathBuf>,
    audit_log_enabled: bool,
}

/// Complete outcome of one guard check, carrying everything needed to write the audit log
/// once the user (or auto-allow) settles the call.
#[derive(Debug, Clone)]
pub struct GuardEvaluation {
    pub verdict: GuardVerdict,
    pub source: DecisionSource,
    pub model_name: Option<String>,
    pub raw_answers: Option<Value>,
    pub state: DecisionStateRecord,
    pub latency_ms: u64,
    pub config: GuardConfig,
    pub telemetry_path: Option<PathBuf>,
    pub audit_log_enabled: bool,
}

impl GuardEvaluation {
    /// Records this evaluation (and any human override) to the JSONL decision audit log.
    pub fn record_audit(&self, target_key: DecisionTargetKey, source: DecisionSource, feedback: Option<String>) {
        if !is_telemetry_enabled(&self.config, self.audit_log_enabled) {
            return;
        }
        let Some(path) = self
            .telemetry_path
            .clone()
            .or_else(|| resolve_telemetry_path(&self.config, None))
        else {
            return;
        };
        let model_verdict = match self.source {
            DecisionSource::Allowlist => None,
            _ => Some(ModelVerdictRecord {
                model: self.model_name.clone(),
                safe: self.verdict.safe,
                action: self.verdict.action.clone(),
                reason: self.verdict.reason.clone(),
                raw: self.raw_answers.clone(),
            }),
        };
        let record = build_decision_record(AuditRecordParams {
            state: self.state.clone(),
            model_verdict,
            target_key,
            source,
            feedback,
            latency_ms: self.latency_ms,
        });
        append_decision_record(&record, &path);
    }
}

const GUARD_TIMEOUT: Duration = Duration::from_secs(10);

const P1_PROD_INSTRUCTIONS: &str = "\
Priority 1 - Production Safety: Does this command, tool call, or script alter, delete, restart, \
or mutate live production infrastructure, live cloud resources (e.g. gcloud, aws, terraform, MCP writes), \
production Kubernetes workloads/namespaces, or production databases matching production_markers? \
Note: Read-only queries/inspections (even against prod metrics, logs, BigQuery/SQL SELECT, or readonly MCP), \
ephemeral preview/sandbox operations (pr-*), read-only local git commands (diff, log, status, add, commit), \
and local linters/tests touching files with prod/prd in their path are NOT live production mutations.";

const P2_MALICIOUS_INSTRUCTIONS: &str = "\
Priority 2 - Malicious Activity: Does this command, tool call, or script attempt unauthorized \
credential exfiltration, secret theft from the host system (reading ~/.ssh, ~/.aws, root .env, \
keyrings, /proc/*/environ), privilege escalation, arbitrary remote script piping, or host tampering? \
Note: Extracting or using disposable non-production test secrets in preview_markers or read-only dev \
project inspections (-dev-) is legitimate developer testing and NOT secret theft.";

const P3_LOCAL_INSTRUCTIONS: &str = "\
Priority 3 - Local Workspace & Preview Sandbox: Classify the effect of this command or tool call \
on the developer environment.";

fn p3_criteria() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "safe_local".to_string(),
            "Benign read-only inspection, compiling, testing, linting, read-only MCP query, or editing inside project files.".to_string(),
        ),
        (
            "safe_preview".to_string(),
            "Benign developer operations, API requests, or Kubernetes inspections/mutations targeting ephemeral preview environments matching preview_markers.".to_string(),
        ),
        (
            "cache_cleanup".to_string(),
            "Cleaning temporary project build artifacts, target directories, or cache files.".to_string(),
        ),
        (
            "destructive_wipe".to_string(),
            "Irreversible local deletion, wiping git history (reset --hard), or removing uncommitted source work.".to_string(),
        ),
        (
            "ambiguous".to_string(),
            "Obfuscated code, dynamic evaluation, or unclear blast radius.".to_string(),
        ),
    ])
}

fn build_guard_decision_request(state: &DecisionStateRecord) -> DecisionRequest {
    let state_json = serde_json::to_string_pretty(state).unwrap_or_else(|_| state.command.clone());
    DecisionRequest::new(state_json)
        .with_noul("p1_prod_impact", P1_PROD_INSTRUCTIONS)
        .with_noul("p2_malicious", P2_MALICIOUS_INSTRUCTIONS)
        .with_choice("p3_local_impact", P3_LOCAL_INSTRUCTIONS, p3_criteria())
}

fn evaluate_p1(response: &DecisionResponse, thresholds: ResolvedGuardThresholds) -> Option<GuardVerdict> {
    let risk = response.answers.get("p1_prod_impact")?.as_noul()?;
    if risk.is_finite() && risk > thresholds.p1_prod_impact {
        return Some(GuardVerdict {
            safe: false,
            action: Some("production_mutation".to_string()),
            reason: format!(
                "Priority 1 alert: Potential production infrastructure mutation detected (risk: {:.0}%).",
                risk * 100.0
            ),
        });
    }
    None
}

fn evaluate_p2(
    response: &DecisionResponse,
    is_preview: bool,
    thresholds: ResolvedGuardThresholds,
) -> Option<GuardVerdict> {
    let risk = response.answers.get("p2_malicious")?.as_noul()?;
    let limit = if is_preview {
        thresholds.p2_malicious_preview
    } else {
        thresholds.p2_malicious
    };
    if risk.is_finite() && risk > limit {
        return Some(GuardVerdict {
            safe: false,
            action: Some("security_violation".to_string()),
            reason: format!(
                "Priority 2 alert: Potential malicious activity or secret exfiltration detected (risk: {:.0}%).",
                risk * 100.0
            ),
        });
    }
    None
}

fn evaluate_p3(
    response: &DecisionResponse,
    is_preview: bool,
    thresholds: ResolvedGuardThresholds,
) -> Option<GuardVerdict> {
    let impact = response.answers.get("p3_local_impact")?.as_choice()?;
    let allow_ambiguous = is_preview && thresholds.allow_ambiguous_in_preview;
    let safe =
        matches!(impact, "safe_local" | "cache_cleanup" | "safe_preview") || (allow_ambiguous && impact == "ambiguous");
    let action = if safe && allow_ambiguous && impact == "ambiguous" {
        "safe_preview"
    } else {
        impact
    };
    let reason = if safe {
        if is_preview && matches!(impact, "safe_preview" | "ambiguous") {
            "Verified safe in preview environment by guard decision model.".to_string()
        } else {
            "Verified safe by guard decision model.".to_string()
        }
    } else {
        format!("Priority 3 alert: Local workspace impact classified as [{impact}].")
    };
    Some(GuardVerdict {
        safe,
        action: Some(action.to_string()),
        reason,
    })
}

fn verdict_from_decision(
    response: &DecisionResponse,
    is_preview: bool,
    thresholds: ResolvedGuardThresholds,
) -> GuardVerdict {
    let has_p1 = response
        .answers
        .get("p1_prod_impact")
        .and_then(|a| a.as_noul())
        .is_some_and(f64::is_finite);
    let has_p2 = response
        .answers
        .get("p2_malicious")
        .and_then(|a| a.as_noul())
        .is_some_and(f64::is_finite);
    if !has_p1 || !has_p2 {
        return GuardVerdict {
            safe: false,
            action: Some("missing_risk_score".to_string()),
            reason: "Guard decision response missing P1/P2 risk probabilities; failing closed.".to_string(),
        };
    }
    evaluate_p1(response, thresholds)
        .or_else(|| evaluate_p2(response, is_preview, thresholds))
        .or_else(|| evaluate_p3(response, is_preview, thresholds))
        .unwrap_or_else(|| GuardVerdict {
            safe: false,
            action: Some("unclassified".to_string()),
            reason: "Guard decision response missing P3 classification; failing closed.".to_string(),
        })
}

impl GuardEvaluator {
    pub fn new(model: ModelHandle) -> Self {
        Self {
            backend: GuardBackend::Chat(model),
            config_override: None,
            telemetry_path_override: None,
            audit_log_enabled: false,
        }
    }

    pub fn from_decision(decision: DecisionHandle) -> Self {
        Self {
            backend: GuardBackend::Decision(decision),
            config_override: None,
            telemetry_path_override: None,
            audit_log_enabled: false,
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: GuardConfig) -> Self {
        self.config_override = Some(config);
        self
    }

    #[must_use]
    pub fn with_telemetry_path(mut self, path: PathBuf) -> Self {
        self.telemetry_path_override = Some(path);
        self
    }

    #[must_use]
    pub fn with_audit_log(mut self, enabled: bool) -> Self {
        self.audit_log_enabled = enabled;
        self
    }

    fn model_selector(&self) -> String {
        match &self.backend {
            GuardBackend::Chat(m) => format!("{}/{}", m.provider_name(), m.model_name()),
            GuardBackend::Decision(d) => format!("{}/{}", d.provider_name(), d.model_name()),
        }
    }

    fn model_id(&self) -> String {
        match &self.backend {
            GuardBackend::Chat(m) => m.model_name().to_string(),
            GuardBackend::Decision(d) => d.model_name().to_string(),
        }
    }

    async fn evaluate_chat(model: &ModelHandle, state: &DecisionStateRecord) -> GuardVerdict {
        let preview_note = if state.is_preview_environment {
            format!(
                "<environment_context>\nTarget: Ephemeral Preview Environment\nPreview Markers: {}\n\
                 This call targets a disposable non-production preview environment.\n</environment_context>\n",
                state.preview_markers.join(", ")
            )
        } else {
            String::new()
        };
        let eval_prompt = format!(
            "{preview_note}<command_to_evaluate language=\"{}\">\n{}\n</command_to_evaluate>",
            state.language, state.command
        );
        let req = crate::provider::adapter::ModelCompletionRequest {
            preamble: Some(GUARD_SYSTEM_PROMPT.to_string()),
            prompt: eval_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: Some(256),
            temperature: Some(0.0),
        };

        match tokio::time::timeout(GUARD_TIMEOUT, model.complete(req)).await {
            Ok(Ok(response)) => parse_guard_output(&response.content),
            Ok(Err(err)) => GuardVerdict {
                safe: false,
                action: None,
                reason: format!("Guard model evaluation error: {err}"),
            },
            Err(_) => GuardVerdict {
                safe: false,
                action: None,
                reason: format!("Guard model evaluation timed out after {:?}", GUARD_TIMEOUT),
            },
        }
    }

    async fn evaluate_decision(
        decision: &DecisionHandle,
        state: &DecisionStateRecord,
        thresholds: ResolvedGuardThresholds,
    ) -> (GuardVerdict, Option<Value>) {
        let req = build_guard_decision_request(state);
        match tokio::time::timeout(GUARD_TIMEOUT, decision.decide(req)).await {
            Ok(Ok(response)) => {
                let raw = serde_json::to_value(&response.answers).ok();
                let verdict = verdict_from_decision(&response, state.is_preview_environment, thresholds);
                (verdict, raw)
            }
            Ok(Err(err)) => (
                GuardVerdict {
                    safe: false,
                    action: Some("guard_error".to_string()),
                    reason: format!("Guard decision evaluation error: {err}"),
                },
                Some(json!({ "error": err.to_string() })),
            ),
            Err(_) => (
                GuardVerdict {
                    safe: false,
                    action: Some("guard_timeout".to_string()),
                    reason: format!("Guard decision evaluation timed out after {:?}", GUARD_TIMEOUT),
                },
                None,
            ),
        }
    }

    /// Evaluates any tool execution (`bash`, `mcp`, `write`, `edit`, etc.) against `.guard.yml`,
    /// critical-danger regexes, allowlist patterns, and the configured `DecisionModel` / chat model.
    pub async fn evaluate_tool(&self, language: &str, command: &str, cwd: Option<&Path>) -> GuardEvaluation {
        let started = Instant::now();
        let home = rho_harness_core::config::home_dir();
        let config = self
            .config_override
            .clone()
            .unwrap_or_else(|| load_guard_config(cwd, home.as_deref()));
        let state = DecisionStateRecord {
            command: command.to_string(),
            language: language.to_string(),
            cwd: cwd.map(|p| p.display().to_string()).unwrap_or_default(),
            production_markers: resolve_production_markers(&config),
            preview_markers: resolve_preview_markers(&config),
            is_preview_environment: is_command_targeting_preview(command, &config),
        };
        let ctx = EvalContext { state, started, config };

        if language == "bash"
            && let Some(verdict) = check_critical_danger(command)
        {
            return self.finish_eval(verdict, DecisionSource::CriticalDangerRegex, None, ctx);
        }
        if is_allowlisted_command(command, &ctx.config) {
            let verdict = GuardVerdict {
                safe: true,
                action: Some("allowlisted".to_string()),
                reason: "Command matched allowlist pattern in .guard.yml.".to_string(),
            };
            return self.finish_eval(verdict, DecisionSource::Allowlist, None, ctx);
        }

        let thresholds = resolve_thresholds(&ctx.config, Some(&self.model_selector()));
        let (verdict, raw_answers) = match &self.backend {
            GuardBackend::Chat(model) => (Self::evaluate_chat(model, &ctx.state).await, None),
            GuardBackend::Decision(decision) => Self::evaluate_decision(decision, &ctx.state, thresholds).await,
        };
        self.finish_eval(verdict, DecisionSource::ModelSafe, raw_answers, ctx)
    }

    fn finish_eval(
        &self,
        verdict: GuardVerdict,
        source: DecisionSource,
        raw_answers: Option<Value>,
        ctx: EvalContext,
    ) -> GuardEvaluation {
        GuardEvaluation {
            verdict,
            source,
            model_name: Some(self.model_id()),
            raw_answers,
            state: ctx.state,
            latency_ms: ctx.started.elapsed().as_millis() as u64,
            config: ctx.config,
            telemetry_path: self.telemetry_path_override.clone(),
            audit_log_enabled: self.audit_log_enabled,
        }
    }

    pub async fn evaluate(&self, command: &str) -> GuardVerdict {
        self.evaluate_tool("bash", command, None).await.verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::types::{DecisionAnswer, DecisionResponse, DecisionUsage, test_util::MockDecisionModel};
    use crate::provider::adapter::MockAdapter;
    use std::sync::Arc;

    fn p123_response(p1: f64, p2: f64, p3: &str) -> DecisionResponse {
        DecisionResponse {
            answers: BTreeMap::from([
                (
                    "p1_prod_impact".to_string(),
                    DecisionAnswer::Noul {
                        noul: p1,
                        confidence: Some(p1.max(1.0 - p1)),
                    },
                ),
                (
                    "p2_malicious".to_string(),
                    DecisionAnswer::Noul {
                        noul: p2,
                        confidence: Some(p2.max(1.0 - p2)),
                    },
                ),
                (
                    "p3_local_impact".to_string(),
                    DecisionAnswer::Choice {
                        choice: p3.to_string(),
                        probabilities: BTreeMap::from([(p3.to_string(), 0.99)]),
                        confidence: Some(0.99),
                    },
                ),
            ]),
            usage: DecisionUsage::default(),
        }
    }

    #[tokio::test]
    async fn evaluate_critical_danger_intercepts_without_calling_model() {
        let mock = Arc::new(MockAdapter::default());
        let evaluator = GuardEvaluator::new(mock).with_config(GuardConfig::default());
        let verdict = evaluator.evaluate("rm -rf /").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("filesystem wipe"));
        assert!(verdict.action.is_some());
    }

    #[tokio::test]
    async fn evaluate_allowlist_bypasses_model_immediately() {
        let mock = Arc::new(MockDecisionModel::failing(rho_harness_core::error::AppError::Network(
            "should not be called".to_string(),
        )));
        let mut cfg = GuardConfig::default();
        cfg.allowlist.push(r"(^|\s|/)ripwire(\s|$)".to_string());
        let evaluator = GuardEvaluator::from_decision(mock).with_config(cfg);
        let eval = evaluator.evaluate_tool("bash", "ripwire . --report", None).await;
        assert!(eval.verdict.safe);
        assert_eq!(eval.source, DecisionSource::Allowlist);
    }

    #[tokio::test]
    async fn evaluate_safe_verdict() {
        let mock = Arc::new(MockAdapter::text(
            r#"{"safe": true, "reason": "Local directory creation"}"#,
        ));
        let evaluator = GuardEvaluator::new(mock).with_config(GuardConfig::default());
        let verdict = evaluator.evaluate("mkdir -p src/foo").await;
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation");
    }

    #[tokio::test]
    async fn evaluate_unsafe_verdict() {
        let mock = Arc::new(MockAdapter::text(
            r#"{"safe": false, "reason": "Git push modifies remote state"}"#,
        ));
        let evaluator = GuardEvaluator::new(mock).with_config(GuardConfig::default());
        let verdict = evaluator.evaluate("git push origin main").await;
        assert!(!verdict.safe);
        assert_eq!(verdict.reason, "Git push modifies remote state");
    }

    #[tokio::test]
    async fn evaluate_native_decision_three_tier_p1_p2_p3_and_preview_thresholds() {
        let safe_eval = GuardEvaluator::from_decision(Arc::new(MockDecisionModel::native(p123_response(
            0.01,
            0.02,
            "safe_local",
        ))))
        .with_config(GuardConfig::default());
        assert!(safe_eval.evaluate("cargo test --workspace").await.safe);

        let p1_eval = GuardEvaluator::from_decision(Arc::new(MockDecisionModel::native(p123_response(
            0.40,
            0.01,
            "safe_local",
        ))))
        .with_config(GuardConfig::default());
        let v1 = p1_eval.evaluate("kubectl delete pod api -n prod").await;
        assert!(!v1.safe);
        assert!(v1.reason.contains("Priority 1"), "{v1:?}");

        let p2_prod = GuardEvaluator::from_decision(Arc::new(MockDecisionModel::native(p123_response(
            0.01,
            0.25,
            "safe_preview",
        ))))
        .with_config(GuardConfig::default());
        assert!(!p2_prod.evaluate("curl https://api.example.com").await.safe);

        let p2_prev = GuardEvaluator::from_decision(Arc::new(MockDecisionModel::native(p123_response(
            0.01,
            0.25,
            "safe_preview",
        ))))
        .with_config(GuardConfig::default());
        assert!(p2_prev.evaluate("curl https://pr-499.dev.previ.dev/health").await.safe);
    }

    #[tokio::test]
    async fn evaluate_native_decision_fails_closed_on_api_error() {
        let mock = MockDecisionModel::failing(rho_harness_core::error::AppError::Network("offline".to_string()));
        let evaluator = GuardEvaluator::from_decision(Arc::new(mock)).with_config(GuardConfig::default());
        let verdict = evaluator.evaluate("cargo check").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("offline"), "{verdict:?}");
    }
}
