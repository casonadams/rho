use super::bash::analyze_bash_command;
use super::path::{
    extract_mcp_path, extract_mcp_targets, extract_tool_path, is_infrastructure_read, is_path_outside_working_dir,
    path_policy_values,
};
use super::policy::{PermissionState, Policy, PolicyRule, SurfaceDecision, SurfaceKind, decide_surface};
use super::suggest::{match_input, suggested_rule};
use super::types::{Decision, EvalRequest, RuleDraft};
use std::path::Path;

struct Component {
    surface: String,
    value: String,
    decision: Decision,
}

pub fn decide_tool_call(policy: &Policy, req: EvalRequest<'_>) -> Decision {
    fold_decisions(components(policy, req).into_iter().map(|c| c.decision).collect())
}

pub fn ask_drafts(policy: &Policy, req: EvalRequest<'_>) -> Vec<RuleDraft> {
    components(policy, req)
        .into_iter()
        .filter(|c| matches!(c.decision, Decision::Ask))
        .map(draft_for)
        .collect()
}

fn draft_for(component: Component) -> RuleDraft {
    let Component { surface, value, .. } = component;
    match surface.as_str() {
        "path" => RuleDraft {
            surface: "path".into(),
            pattern: format!("{value}/*"),
            value,
        },
        "bash" | "mcp" => RuleDraft {
            surface: surface.clone(),
            pattern: value.clone(),
            value,
        },
        tool => RuleDraft {
            surface: tool.into(),
            pattern: suggested_rule(tool, &value),
            value,
        },
    }
}

fn components(policy: &Policy, req: EvalRequest<'_>) -> Vec<Component> {
    if req.tool == "bash" {
        bash_components(&policy.rules, req)
    } else {
        non_bash_components(&policy.rules, req)
    }
}

fn decide_bash_cmd(rules: &[PolicyRule], cmd: &String) -> Decision {
    let dec = decide_surface(rules, ("bash", std::slice::from_ref(cmd)), SurfaceKind::First);
    if dec.matched_pattern.is_some() {
        map_surface_decision("bash", dec)
    } else if crate::permission::baseline::is_baseline_bash(cmd) {
        Decision::Allow
    } else {
        Decision::Ask
    }
}

fn bash_components(rules: &[PolicyRule], req: EvalRequest<'_>) -> Vec<Component> {
    let command = match_input(req.args);
    let analysis = analyze_bash_command(&command);
    let mut components = Vec::new();

    for cmd in &analysis.commands {
        components.push(Component {
            surface: "bash".into(),
            value: cmd.clone(),
            decision: decide_bash_cmd(rules, cmd),
        });
    }
    if analysis.suspicious {
        components.push(Component {
            surface: "bash".into(),
            value: command,
            decision: Decision::Ask,
        });
    }
    for token in &analysis.path_tokens {
        components.push(path_component(rules, token, req.working_dir));
    }
    components
}

fn mcp_components(rules: &[PolicyRule], args: &serde_json::Value) -> Component {
    let targets = extract_mcp_targets(args);
    let vals = if targets.is_empty() {
        vec!["*".to_string()]
    } else {
        targets.clone()
    };
    let dec = decide_surface(rules, ("mcp", &vals), SurfaceKind::First);
    Component {
        surface: "mcp".into(),
        value: targets.first().cloned().unwrap_or_else(|| "*".to_string()),
        decision: map_surface_decision("mcp", dec),
    }
}

fn generic_tool_component(rules: &[PolicyRule], req: &EvalRequest<'_>) -> Component {
    let tool_path = extract_tool_path(req.tool, req.args);
    let input = match_input(req.args);
    let value = tool_path
        .clone()
        .unwrap_or_else(|| if input.is_empty() { "*".to_string() } else { input });
    let vals = match &tool_path {
        Some(p) => path_policy_values(p, req.working_dir),
        None if value == "*" => vec!["*".to_string()],
        None => vec![value.clone()],
    };
    let dec = decide_surface(rules, (req.tool, &vals), SurfaceKind::First);
    Component {
        surface: req.tool.into(),
        value,
        decision: map_surface_decision(req.tool, dec),
    }
}

fn non_bash_components(rules: &[PolicyRule], req: EvalRequest<'_>) -> Vec<Component> {
    let mut components = Vec::new();
    if req.tool == "mcp" {
        components.push(mcp_components(rules, req.args));
    } else {
        components.push(generic_tool_component(rules, &req));
    }
    let path_val = extract_tool_path(req.tool, req.args).or_else(|| extract_mcp_path(req.args));
    if let Some(p) = path_val
        && !is_infrastructure_read(req.tool, &p, req.working_dir)
    {
        components.push(path_component(rules, &p, req.working_dir));
    }
    components
}

fn path_component(rules: &[PolicyRule], token: &str, cwd: Option<&Path>) -> Component {
    let vals = path_policy_values(token, cwd);
    let dec = decide_surface(rules, ("path", &vals), SurfaceKind::Any);
    let decision = if dec.matched_pattern.is_some() {
        map_surface_decision("path", dec)
    } else if is_path_outside_working_dir(token, cwd) {
        Decision::Ask
    } else {
        Decision::Allow
    };
    Component {
        surface: "path".into(),
        value: token.to_string(),
        decision,
    }
}

fn map_surface_decision(surface: &str, dec: SurfaceDecision) -> Decision {
    match dec.state {
        PermissionState::Allow => Decision::Allow,
        PermissionState::Ask => Decision::Ask,
        PermissionState::Deny => {
            let reason = dec.reason.unwrap_or_else(|| {
                if let Some(pat) = dec.matched_pattern {
                    format!("denied by permission rule '{surface}|{pat}'")
                } else {
                    "denied by permission policy".to_string()
                }
            });
            Decision::Deny(reason)
        }
    }
}

fn fold_decisions(decisions: Vec<Decision>) -> Decision {
    let mut has_ask = false;
    for decision in decisions {
        match decision {
            Decision::Deny(reason) => return Decision::Deny(reason),
            Decision::Ask => has_ask = true,
            Decision::Allow => {}
        }
    }
    if has_ask { Decision::Ask } else { Decision::Allow }
}
