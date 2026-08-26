use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct EdgeCase {
    pub scenario: String,
    #[serde(rename = "expectedBehavior")]
    pub expected_behavior: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct EvidenceItem {
    #[serde(rename = "type")]
    pub evidence_type: String,
    pub source: String,
    pub excerpt: String,
    #[serde(default)]
    pub anchors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct ScopeDefinition {
    #[serde(default, rename = "inScope")]
    pub in_scope: Vec<String>,
    #[serde(default, rename = "outOfScope")]
    pub out_of_scope: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct IntentSpec {
    pub id: String,
    #[serde(default = "default_status")]
    pub status: String,
    pub objective: String,
    #[serde(rename = "userGoal", skip_serializing_if = "Option::is_none")]
    pub user_goal: Option<String>,
    #[serde(default)]
    pub outcomes: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub scope: Option<ScopeDefinition>,
    #[serde(default, rename = "edgeCases")]
    pub edge_cases: Vec<EdgeCase>,
    #[serde(default)]
    pub verification: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
}

fn default_status() -> String {
    "approved".to_string()
}

impl IntentSpec {
    pub fn from_prompt(prompt: &str) -> Self {
        Self {
            id: format!("INT-{}", uuid::Uuid::new_v4().to_string()[..8].to_uppercase()),
            status: "approved".to_string(),
            objective: prompt.trim().to_string(),
            user_goal: Some(prompt.trim().to_string()),
            outcomes: Vec::new(),
            constraints: Vec::new(),
            scope: None,
            edge_cases: Vec::new(),
            verification: Vec::new(),
            evidence: Vec::new(),
        }
    }

    pub fn parse_markdown(content: &str) -> Option<Self> {
        let trimmed = content.trim();
        if !trimmed.starts_with("---") {
            return None;
        }
        let remainder = &trimmed[3..];
        let end_idx = remainder.find("---")?;
        let yaml_str = &remainder[..end_idx];
        serde_yaml::from_str::<Self>(yaml_str).ok()
    }

    pub fn to_system_prompt_section(&self) -> String {
        let mut out = String::new();
        out.push_str("## Structured Intent & Guardrails (IntentSpec)\n");
        out.push_str(&format!("- **Objective**: {}\n", self.objective));
        if !self.outcomes.is_empty() {
            out.push_str("- **Target Outcomes**:\n");
            for o in &self.outcomes {
                out.push_str(&format!("  * {o}\n"));
            }
        }
        if !self.constraints.is_empty() {
            out.push_str("- **Constraints**:\n");
            for c in &self.constraints {
                out.push_str(&format!("  * {c}\n"));
            }
        }
        if let Some(ref sc) = self.scope {
            if !sc.in_scope.is_empty() {
                out.push_str("- **In Scope**:\n");
                for s in &sc.in_scope {
                    out.push_str(&format!("  * {s}\n"));
                }
            }
            if !sc.out_of_scope.is_empty() {
                out.push_str("- **Out of Scope**:\n");
                for s in &sc.out_of_scope {
                    out.push_str(&format!("  * {s}\n"));
                }
            }
        }
        if !self.edge_cases.is_empty() {
            out.push_str("- **Edge Cases**:\n");
            for e in &self.edge_cases {
                out.push_str(&format!(
                    "  * Scenario: {} -> Expected: {}\n",
                    e.scenario, e.expected_behavior
                ));
            }
        }
        if !self.verification.is_empty() {
            out.push_str("- **Verification Obligations**:\n");
            for v in &self.verification {
                out.push_str(&format!("  * {v}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_spec_parse_yaml() {
        let md = r#"---
id: "INT-TEST-001"
status: "approved"
objective: "Add search caching"
outcomes:
  - "Cache responses for 60s"
constraints:
  - "Use in-memory LRU"
---
# Some other notes
"#;
        let parsed = IntentSpec::parse_markdown(md).unwrap();
        assert_eq!(parsed.id, "INT-TEST-001");
        assert_eq!(parsed.objective, "Add search caching");
        assert_eq!(parsed.outcomes.len(), 1);
        assert_eq!(parsed.constraints.len(), 1);
    }
}
