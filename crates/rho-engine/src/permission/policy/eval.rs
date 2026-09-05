use super::model::{PermissionState, Policy, PolicyRule, ScopeRules, SurfaceDecision, SurfaceKind};
use crate::permission::baseline::{BASELINE_BASH_ALLOW, BASELINE_TOOLS};
use crate::permission::matcher::wildcard_match;

pub fn build_policy(global: Option<ScopeRules>, project: Option<ScopeRules>) -> Policy {
    let global_scope = global.unwrap_or_default();
    let project_scope = project.unwrap_or_default();
    let merged = merge_scope_rules(&global_scope.rules, &project_scope.rules);
    let universal = project_scope
        .universal
        .or(global_scope.universal)
        .unwrap_or(PermissionState::Ask);

    let mut rules = Vec::new();
    rules.push(PolicyRule {
        surface: "*".into(),
        pattern: "*".into(),
        state: universal,
        reason: None,
        synthetic: true,
    });

    add_catchalls_and_baselines(&merged, &mut rules);
    for rule in merged.into_iter().filter(|r| r.pattern != "*") {
        rules.push(rule);
    }
    Policy { rules }
}

fn add_catchalls_and_baselines(merged: &[PolicyRule], rules: &mut Vec<PolicyRule>) {
    for rule in merged.iter().filter(|r| r.pattern == "*") {
        rules.push(rule.clone());
    }
    for tool in BASELINE_TOOLS {
        if !merged.iter().any(|r| r.surface == *tool) {
            rules.push(PolicyRule {
                surface: tool.to_string(),
                pattern: "*".into(),
                state: PermissionState::Allow,
                reason: None,
                synthetic: true,
            });
        }
    }
    for pattern in BASELINE_BASH_ALLOW {
        rules.push(PolicyRule {
            surface: "bash".into(),
            pattern: pattern.to_string(),
            state: PermissionState::Allow,
            reason: None,
            synthetic: true,
        });
    }
}

fn merge_scope_rules(base: &[PolicyRule], override_rules: &[PolicyRule]) -> Vec<PolicyRule> {
    let mut result = base.to_vec();
    for rule in override_rules {
        if let Some(pos) = result
            .iter()
            .position(|r| r.surface == rule.surface && r.pattern == rule.pattern)
        {
            result[pos] = rule.clone();
        } else {
            result.push(rule.clone());
        }
    }
    result
}

pub fn decide_surface(rules: &[PolicyRule], target: (&str, &[String]), kind: SurfaceKind) -> SurfaceDecision {
    let (surface, values) = target;
    if kind == SurfaceKind::Any {
        return last_match(rules, surface, values).unwrap_or(SurfaceDecision {
            state: PermissionState::Ask,
            reason: None,
            matched_pattern: None,
        });
    }
    for val in values {
        if let Some(match_dec) = last_match(rules, surface, std::slice::from_ref(val))
            && match_dec.matched_pattern.is_some()
        {
            return match_dec;
        }
    }
    let fallback_val = values.first().map(String::as_str).unwrap_or("*");
    last_match(rules, surface, &[fallback_val.to_string()]).unwrap_or(SurfaceDecision {
        state: PermissionState::Ask,
        reason: None,
        matched_pattern: None,
    })
}

fn last_match(rules: &[PolicyRule], surface: &str, values: &[String]) -> Option<SurfaceDecision> {
    for rule in rules.iter().rev() {
        if !rule_surface_matches(&rule.surface, surface) {
            continue;
        }
        if !values.iter().any(|v| wildcard_match(&rule.pattern, v)) {
            continue;
        }
        return Some(SurfaceDecision {
            state: rule.state,
            reason: rule.reason.clone(),
            matched_pattern: if rule.synthetic {
                None
            } else {
                Some(rule.pattern.clone())
            },
        });
    }
    None
}

fn rule_surface_matches(rule_surface: &str, target_surface: &str) -> bool {
    rule_surface == "*" || rule_surface == target_surface || wildcard_match(rule_surface, target_surface)
}
