use super::super::IntentSpec;
use crate::error::{AppError, Result};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct VerificationResult {
    pub obligation: String,
    #[serde(deserialize_with = "deserialize_bool")]
    pub passed: bool,
}

pub(super) fn validate(spec: &IntentSpec, progress: &IntentProgress) -> Result<()> {
    if !progress.complete {
        return Ok(());
    }
    let missing_outcome = spec
        .outcomes
        .iter()
        .any(|required| !progress.completed_outcomes.contains(required));
    let failed_verification = spec.verification.iter().any(|required| {
        !progress
            .verification
            .iter()
            .any(|result| result.obligation == *required && result.passed)
    });
    if missing_outcome || failed_verification {
        return Err(AppError::Intent(
            "Intent cannot complete with pending outcomes or verification".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct IntentProgress {
    #[serde(default)]
    pub completed_outcomes: Vec<String>,
    #[serde(default)]
    pub verification: Vec<VerificationResult>,
    pub blocked_reason: Option<String>,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub complete: bool,
}

fn deserialize_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolValue {
        Bool(bool),
        Integer(i64),
        String(String),
    }

    match BoolValue::deserialize(deserializer)? {
        BoolValue::Bool(value) => Ok(value),
        BoolValue::Integer(1) => Ok(true),
        BoolValue::Integer(0) => Ok(false),
        BoolValue::String(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(true),
        BoolValue::String(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(false),
        BoolValue::Integer(value) => Err(serde::de::Error::custom(format!("invalid boolean integer: {value}"))),
        BoolValue::String(value) => Err(serde::de::Error::custom(format!("invalid boolean string: {value}"))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentDecision {
    pub key: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> IntentSpec {
        let mut s = IntentSpec::from_prompt("fix auth");
        s.outcomes.push("Regression test passes".to_string());
        s.verification.push("cargo test auth".to_string());
        s
    }

    #[test]
    fn progress_accepts_common_boolean_encodings() {
        for (complete, passed, expected) in [
            (serde_json::json!(true), serde_json::json!("true"), true),
            (serde_json::json!("1"), serde_json::json!(1), true),
            (serde_json::json!("false"), serde_json::json!("0"), false),
        ] {
            let progress: IntentProgress = serde_json::from_value(serde_json::json!({
                "completed_outcomes": [],
                "verification": [{"obligation": "check", "passed": passed}],
                "blocked_reason": null,
                "complete": complete
            }))
            .unwrap();
            assert_eq!(progress.complete, expected);
            assert_eq!(progress.verification[0].passed, expected);
        }
    }

    #[test]
    fn progress_rejects_unknown_boolean_strings() {
        let result = serde_json::from_value::<IntentProgress>(serde_json::json!({
            "completed_outcomes": [],
            "verification": [],
            "blocked_reason": null,
            "complete": "yes"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_completion_without_outcomes() {
        let s = spec();
        let progress = IntentProgress {
            completed_outcomes: Vec::new(),
            verification: Vec::new(),
            blocked_reason: None,
            complete: true,
        };
        assert!(validate(&s, &progress).is_err());
    }

    #[test]
    fn validate_rejects_completion_with_failed_verification() {
        let s = spec();
        let progress = IntentProgress {
            completed_outcomes: vec!["Regression test passes".to_string()],
            verification: vec![VerificationResult {
                obligation: "cargo test auth".to_string(),
                passed: false,
            }],
            blocked_reason: None,
            complete: true,
        };
        assert!(validate(&s, &progress).is_err());
    }

    #[test]
    fn validate_accepts_incomplete_progress_regardless_of_outcomes() {
        let s = spec();
        let progress = IntentProgress {
            completed_outcomes: Vec::new(),
            verification: Vec::new(),
            blocked_reason: None,
            complete: false,
        };
        assert!(validate(&s, &progress).is_ok());
    }
}
