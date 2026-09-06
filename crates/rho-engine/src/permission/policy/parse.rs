use super::model::{PermissionState, PolicyRule, RawConfigFile, RuleAction, ScopeRules};

pub fn parse_scope_from_str(raw: &str) -> Result<ScopeRules, String> {
    let parsed: RawConfigFile = toml::from_str(raw).map_err(|e| e.to_string())?;
    Ok(normalize_raw_config(parsed))
}

fn normalize_raw_config(config: RawConfigFile) -> ScopeRules {
    let mut rules = Vec::new();
    let mut universal = None;

    collect_legacy_rules(&config, &mut rules);
    for (surface, action) in config.permission {
        if surface == "*" {
            universal = parse_universal_action(&action);
            continue;
        }
        collect_surface_rules(&surface, action, &mut rules);
    }
    ScopeRules { rules, universal }
}

fn parse_universal_action(action: &RuleAction) -> Option<PermissionState> {
    match action {
        RuleAction::State(state) => Some(*state),
        _ => None,
    }
}

fn collect_surface_rules(surface: &str, action: RuleAction, rules: &mut Vec<PolicyRule>) {
    match action {
        RuleAction::State(state) => {
            rules.push(PolicyRule {
                surface: surface.to_string(),
                pattern: "*".to_string(),
                state,
                reason: None,
                synthetic: false,
            });
        }
        RuleAction::DenyObject { reason, .. } => {
            rules.push(PolicyRule {
                surface: surface.to_string(),
                pattern: "*".to_string(),
                state: PermissionState::Deny,
                reason,
                synthetic: false,
            });
        }
        RuleAction::SurfaceMap(patterns) => {
            for (pattern, pat_action) in patterns {
                push_pattern_rule((surface, &pattern), pat_action, rules);
            }
        }
    }
}

fn push_pattern_rule(target: (&str, &str), action: RuleAction, rules: &mut Vec<PolicyRule>) {
    let (surface, pattern) = target;
    match action {
        RuleAction::State(state) => {
            rules.push(PolicyRule {
                surface: surface.to_string(),
                pattern: pattern.to_string(),
                state,
                reason: None,
                synthetic: false,
            });
        }
        RuleAction::DenyObject { reason, .. } => {
            rules.push(PolicyRule {
                surface: surface.to_string(),
                pattern: pattern.to_string(),
                state: PermissionState::Deny,
                reason,
                synthetic: false,
            });
        }
        RuleAction::SurfaceMap(_) => {}
    }
}

fn push_legacy_rules(
    map: &std::collections::BTreeMap<String, Vec<String>>,
    state: PermissionState,
    rules: &mut Vec<PolicyRule>,
) {
    for (tool, patterns) in map {
        for pattern in patterns {
            rules.push(PolicyRule {
                surface: tool.clone(),
                pattern: pattern.clone(),
                state,
                reason: None,
                synthetic: false,
            });
        }
    }
}

fn collect_legacy_rules(config: &RawConfigFile, rules: &mut Vec<PolicyRule>) {
    push_legacy_rules(&config.allow, PermissionState::Allow, rules);
    push_legacy_rules(&config.ask, PermissionState::Ask, rules);
    push_legacy_rules(&config.deny, PermissionState::Deny, rules);
}
