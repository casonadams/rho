//! Append-only JSONL decision audit log for guard evaluations.
//!
//! Records every allowlist hit, critical-danger intercept, model verdict (with raw
//! decision probabilities), and human override (`user_proceed` / `user_cancel` /
//! `user_feedback` / `headless_block`) in the same schema as `guard@casonadams-plugins`
//! (`~/.config/rho/guard/decisions.jsonl`).

use super::config::GuardConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_JEV_QUESTION: &str = "Classify the safety policy for this tool execution.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTargetKey {
    SafeAutoAllow,
    AskConfirmation,
    HardDeny,
}

impl DecisionTargetKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::SafeAutoAllow => "A",
            Self::AskConfirmation => "B",
            Self::HardDeny => "C",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Allowlist,
    CriticalDangerRegex,
    ModelSafe,
    UserProceed,
    UserCancel,
    UserFeedback,
    HeadlessBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecisionOption {
    pub label: String,
    pub key: DecisionTargetKey,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionStateRecord {
    pub command: String,
    pub language: String,
    pub cwd: String,
    pub production_markers: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preview_markers: Vec<String>,
    pub is_preview_environment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVerdictRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTargetRecord {
    pub label: String,
    pub key: DecisionTargetKey,
    pub source: DecisionSource,
    pub feedback: Option<String>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecisionRecord {
    pub id: String,
    pub timestamp: String,
    pub state: DecisionStateRecord,
    pub question: String,
    pub options: Vec<JevDecisionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_verdict: Option<ModelVerdictRecord>,
    pub target: DecisionTargetRecord,
}

pub fn default_options() -> Vec<JevDecisionOption> {
    vec![
        JevDecisionOption {
            label: "A".to_string(),
            key: DecisionTargetKey::SafeAutoAllow,
            description: "Benign local operation; auto-execute without prompting.".to_string(),
        },
        JevDecisionOption {
            label: "B".to_string(),
            key: DecisionTargetKey::AskConfirmation,
            description: "Risky, state-altering, or ambiguous; prompt user for confirmation.".to_string(),
        },
        JevDecisionOption {
            label: "C".to_string(),
            key: DecisionTargetKey::HardDeny,
            description: "Violates safety policy or malicious; deny execution.".to_string(),
        },
    ]
}

pub fn is_telemetry_enabled(config: &GuardConfig, config_opt_in: bool) -> bool {
    for var in ["RHO_GUARD_TELEMETRY_DISABLED", "GUARD_TELEMETRY_DISABLED"] {
        if let Ok(v) = std::env::var(var)
            && matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true")
        {
            return false;
        }
    }
    if let Some(explicit) = config.telemetry.enabled {
        return explicit;
    }
    if config_opt_in || config.telemetry.path.is_some() {
        return true;
    }
    for var in ["RHO_GUARD_TELEMETRY_PATH", "GUARD_TELEMETRY_PATH"] {
        if std::env::var(var).is_ok_and(|p| !p.trim().is_empty()) {
            return true;
        }
    }
    false
}

pub fn resolve_telemetry_path(config: &GuardConfig, config_dir: Option<&Path>) -> Option<PathBuf> {
    for var in ["RHO_GUARD_TELEMETRY_PATH", "GUARD_TELEMETRY_PATH"] {
        if let Ok(path) = std::env::var(var)
            && !path.trim().is_empty()
        {
            return Some(PathBuf::from(path.trim()));
        }
    }
    if let Some(path) = config.telemetry.path.as_deref().filter(|p| !p.trim().is_empty()) {
        return Some(PathBuf::from(path.trim()));
    }
    config_dir
        .map(Path::to_path_buf)
        .or_else(rho_harness_core::config::config_dir)
        .map(|d| d.join("guard").join("decisions.jsonl"))
}

pub struct AuditRecordParams {
    pub state: DecisionStateRecord,
    pub model_verdict: Option<ModelVerdictRecord>,
    pub target_key: DecisionTargetKey,
    pub source: DecisionSource,
    pub feedback: Option<String>,
    pub latency_ms: u64,
}

pub fn build_decision_record(params: AuditRecordParams) -> JevDecisionRecord {
    JevDecisionRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        state: params.state,
        question: DEFAULT_JEV_QUESTION.to_string(),
        options: default_options(),
        model_verdict: params.model_verdict,
        target: DecisionTargetRecord {
            label: params.target_key.label().to_string(),
            key: params.target_key,
            source: params.source,
            feedback: params.feedback,
            latency_ms: params.latency_ms,
        },
    }
}

/// Appends one JSONL audit line to `path`, creating parent directories as needed.
/// Errors are suppressed so audit I/O never interrupts tool execution.
pub fn append_decision_record(record: &JevDecisionRecord, path: &Path) {
    let Ok(line) = serde_json::to_string(record) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    #[test]
    fn appends_valid_jsonl_decision_record() {
        let dir = TestDir::with_prefix("rho-guard-audit");
        let path = dir.join("guard/decisions.jsonl");
        let record = build_decision_record(AuditRecordParams {
            state: DecisionStateRecord {
                command: "cargo test".to_string(),
                language: "bash".to_string(),
                cwd: "/workspace".to_string(),
                production_markers: vec!["prod".to_string()],
                preview_markers: vec!["pr-".to_string()],
                is_preview_environment: false,
            },
            model_verdict: Some(ModelVerdictRecord {
                model: Some("gemini-decision-1".to_string()),
                safe: true,
                action: Some("safe_local".to_string()),
                reason: "Verified safe".to_string(),
                raw: Some(serde_json::json!({"p1_prod_impact": {"noul": 0.01}})),
            }),
            target_key: DecisionTargetKey::SafeAutoAllow,
            source: DecisionSource::ModelSafe,
            feedback: None,
            latency_ms: 142,
        });
        append_decision_record(&record, &path);

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: JevDecisionRecord = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.state.command, "cargo test");
        assert_eq!(parsed.target.label, "A");
        assert_eq!(parsed.target.key, DecisionTargetKey::SafeAutoAllow);
        assert_eq!(parsed.target.source, DecisionSource::ModelSafe);
        assert_eq!(parsed.target.latency_ms, 142);
    }
}
