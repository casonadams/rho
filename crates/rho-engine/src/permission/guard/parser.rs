use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static THINK_TAG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<think>.*?</think>").expect("valid think regex"));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardVerdict {
    pub safe: bool,
    pub reason: String,
}

pub fn parse_guard_output(text: &str) -> GuardVerdict {
    let clean = THINK_TAG_REGEX.replace_all(text, "").trim().to_string();

    if let (Some(start), Some(end)) = (clean.find('{'), clean.rfind('}'))
        && end > start
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&clean[start..=end])
        && let Some(safe) = val.get("safe").and_then(serde_json::Value::as_bool)
    {
        let reason = val
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .unwrap_or(if safe {
                "Command verified safe."
            } else {
                "Potential security risk detected."
            })
            .to_string();
        return GuardVerdict { safe, reason };
    }

    let lower = clean.to_ascii_lowercase();
    let is_safe = (lower.contains("\"safe\": true") || lower.contains("safe: true")) && !lower.contains("unsafe");
    let fallback_reason = clean
        .replace("```json", "")
        .replace("```", "")
        .replace(['{', '}', '"'], "")
        .trim()
        .to_string();

    GuardVerdict {
        safe: is_safe,
        reason: if fallback_reason.is_empty() {
            "Action requires human approval.".to_string()
        } else {
            fallback_reason
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_safe() {
        let input = r#"{"safe": true, "reason": "Local directory creation"}"#;
        let verdict = parse_guard_output(input);
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation");
    }

    #[test]
    fn parses_clean_json_unsafe() {
        let input = r#"{"safe": false, "reason": "Remote push modifies remote git state"}"#;
        let verdict = parse_guard_output(input);
        assert!(!verdict.safe);
        assert_eq!(verdict.reason, "Remote push modifies remote git state");
    }

    #[test]
    fn strips_thinking_tags_and_markdown_blocks() {
        let input = r#"
<think>
Evaluating `mkdir -p test`...
This is a standard local workspace file operation under <safe_operations>.
</think>
```json
{
  "safe": true,
  "reason": "Local directory creation is safe"
}
```
"#;
        let verdict = parse_guard_output(input);
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation is safe");
    }

    #[test]
    fn parses_json_with_missing_reason() {
        let input = r#"{"safe": true}"#;
        let verdict = parse_guard_output(input);
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Command verified safe.");

        let input_unsafe = r#"{"safe": false}"#;
        let verdict_unsafe = parse_guard_output(input_unsafe);
        assert!(!verdict_unsafe.safe);
        assert_eq!(verdict_unsafe.reason, "Potential security risk detected.");
    }

    #[test]
    fn heuristic_fallback_recovers_loose_output() {
        let input = "safe: true\nReason: Standard cargo test run";
        let verdict = parse_guard_output(input);
        assert!(verdict.safe);

        let input_malformed = "I do not think this is safe to run without confirmation";
        let verdict_malformed = parse_guard_output(input_malformed);
        assert!(!verdict_malformed.safe);
    }
}
