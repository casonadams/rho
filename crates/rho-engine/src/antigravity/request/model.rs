use serde_json::{Value, json};

/// Normalized thinking effort: off/minimal/low/medium/high. rho's xhigh/max
/// map to high (the backend advertises no finer level).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

impl Effort {
    /// Parse rho's thinking level (see `THINKING_LEVELS` in the REPL).
    pub fn parse(level: Option<&str>) -> Self {
        match level.unwrap_or("off").trim().to_ascii_lowercase().as_str() {
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" | "xhigh" | "max" => Self::High,
            _ => Self::Off,
        }
    }
}

/// Public selectable model ids + thinking effort → backend runtime ids.
/// Static table mirroring pi-antigravity's routing; unknown ids (including
/// already-runtime ids like `gemini-3.7-flash-high`) pass through untouched.
/// ponytail: static table — extend when Google advertises new families.
pub fn resolve_runtime_model(public_id: &str, effort: Effort) -> String {
    match (public_id, effort) {
        ("claude-opus-4-6", _) => "claude-opus-4-6-thinking".to_string(),
        ("gpt-oss-120b", _) => "gpt-oss-120b-medium".to_string(),
        ("gemini-3.5-flash", Effort::High) => "gemini-3-flash-agent".to_string(),
        ("gemini-3.5-flash", Effort::Medium) => "gemini-3.5-flash-low".to_string(),
        ("gemini-3.5-flash", _) => "gemini-3.5-flash-extra-low".to_string(),
        ("gemini-3.1-pro", Effort::High) => "gemini-pro-agent".to_string(),
        ("gemini-3.1-pro", _) => "gemini-3.1-pro-low".to_string(),
        (family @ ("gemini-3.8-flash" | "gemini-3.7-flash" | "gemini-3.6-flash"), effort) => {
            format!("{family}-{}", level_suffix(effort))
        }
        (other, _) => other.to_string(),
    }
}

fn level_suffix(effort: Effort) -> &'static str {
    match effort {
        Effort::Medium => "medium",
        Effort::High => "high",
        _ => "low",
    }
}

/// Collapse a runtime id into its public family id + advertised thinking
/// level (pi parity: tiered variants and agent aliases fold into one family).
/// Suffix order matters: extra-* must be stripped before plain low/high.
pub fn collapse_runtime_id(runtime: &str) -> (String, Option<Effort>) {
    match runtime {
        "gemini-3-flash-agent" => return ("gemini-3.5-flash".to_string(), Some(Effort::High)),
        "gemini-pro-agent" => return ("gemini-3.1-pro".to_string(), Some(Effort::High)),
        _ => {}
    }
    for (suffix, level) in [
        ("extra-low", Some(Effort::Low)),
        ("extra-high", Some(Effort::High)),
        ("thinking", Some(Effort::High)),
        ("minimal", Some(Effort::Minimal)),
        ("medium", Some(Effort::Medium)),
        ("high", Some(Effort::High)),
        ("low", Some(Effort::Low)),
        ("tiered", None),
    ] {
        if let Some(base) = runtime.strip_suffix(suffix)
            && base.ends_with('-')
        {
            return (base.trim_end_matches('-').to_string(), level);
        }
    }
    (runtime.to_string(), None)
}

/// Next-generation fallback when a runtime id 404s (pi parity: 3.8 → 3.7 → 3.6).
pub fn fallback_runtime_model(runtime: &str) -> Option<String> {
    if let Some(rest) = runtime.strip_prefix("gemini-3.8-flash-") {
        return Some(format!("gemini-3.7-flash-{rest}"));
    }
    if runtime == "gemini-3.8-flash" {
        return Some("gemini-3.7-flash-low".to_string());
    }
    if let Some(rest) = runtime.strip_prefix("gemini-3.7-flash-") {
        return Some(format!("gemini-3.6-flash-{rest}"));
    }
    if runtime == "gemini-3.7-flash" {
        return Some("gemini-3.6-flash-low".to_string());
    }
    None
}

pub fn max_output_tokens_cap(runtime: &str) -> u64 {
    if runtime.starts_with("claude-") {
        64000
    } else if runtime.starts_with("gpt-oss-") {
        32768
    } else if runtime.starts_with("gemini-") {
        65535
    } else {
        8192
    }
}

/// Verified backend caps per runtime id; requesting more returns 400.
pub fn cap_max_tokens(runtime: &str, requested: Option<u64>) -> u64 {
    let cap = max_output_tokens_cap(runtime);
    requested.map(|t| t.min(cap)).unwrap_or(cap)
}

pub fn gemini_requires_thought_signature(runtime: &str) -> bool {
    let Some(rest) = runtime.strip_prefix("gemini-") else {
        return false;
    };
    let major: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    major.parse::<u32>().map(|v| v >= 3).unwrap_or(true)
}

pub fn needs_function_call_id(runtime: &str) -> bool {
    runtime.starts_with("claude-") || runtime.starts_with("gpt-oss-")
}

pub fn sanitize_tool_call_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.chars().take(64).collect()
}

/// Model enum labels the backend expects for runtime ids (pi-antigravity parity).
pub fn model_enum_label(runtime: &str) -> Option<&'static str> {
    match runtime {
        // Gemini 3.8 Flash
        "gemini-3.8-flash" | "gemini-3.8-flash-high" => Some("MODEL_PLACEHOLDER_M318"),
        "gemini-3.8-flash-medium" => Some("MODEL_PLACEHOLDER_M319"),
        "gemini-3.8-flash-low" => Some("MODEL_PLACEHOLDER_M320"),
        "gemini-3.8-flash-tiered" => Some("MODEL_PLACEHOLDER_M322"),
        // Gemini 3.7 Flash
        "gemini-3.7-flash" | "gemini-3.7-flash-high" => Some("MODEL_PLACEHOLDER_M298"),
        "gemini-3.7-flash-medium" => Some("MODEL_PLACEHOLDER_M299"),
        "gemini-3.7-flash-low" => Some("MODEL_PLACEHOLDER_M300"),
        "gemini-3.7-flash-tiered" => Some("MODEL_PLACEHOLDER_M301"),
        // Gemini 3.6 Flash
        "gemini-3.6-flash" | "gemini-3.6-flash-high" => Some("MODEL_PLACEHOLDER_M71"),
        "gemini-3.6-flash-medium" => Some("MODEL_PLACEHOLDER_M72"),
        "gemini-3.6-flash-low" => Some("MODEL_PLACEHOLDER_M73"),
        "gemini-3.6-flash-tiered" => Some("MODEL_PLACEHOLDER_M196"),
        // Gemini 3.5 Flash
        "gemini-3.5-flash" | "gemini-3.5-flash-low" => Some("MODEL_PLACEHOLDER_M20"),
        "gemini-3.5-flash-extra-low" => Some("MODEL_PLACEHOLDER_M187"),
        "gemini-3-flash-agent" => Some("MODEL_PLACEHOLDER_M84"),
        // Gemini 3.1 Pro
        "gemini-3.1-pro" | "gemini-3.1-pro-low" => Some("MODEL_PLACEHOLDER_M36"),
        "gemini-3.1-pro-high" => Some("MODEL_PLACEHOLDER_M37"),
        "gemini-pro-agent" => Some("MODEL_PLACEHOLDER_M16"),
        // Claude
        "claude-sonnet-4-6" => Some("MODEL_PLACEHOLDER_M35"),
        "claude-opus-4-6" | "claude-opus-4-6-thinking" => Some("MODEL_PLACEHOLDER_M26"),
        // GPT-OSS
        "gpt-oss-120b" | "gpt-oss-120b-medium" => Some("MODEL_OPENAI_GPT_OSS_120B_MEDIUM"),
        _ => None,
    }
}

/// True when the runtime family wants the Claude interleaved-thinking beta
/// header enabled for the effort.
pub fn wants_claude_thinking_header(runtime_model: &str, effort: Effort) -> bool {
    effort != Effort::Off && runtime_model.starts_with("claude-")
}

/// Gemini/Claude/GPT-OSS thinkingConfig for the effort (pi-antigravity parity).
/// All supported models send integer `thinkingBudget` matching Cloud Code Assist's
/// official wire protocol.
pub fn thinking_config(runtime_model: &str, effort: Effort) -> Value {
    if !runtime_model.starts_with("gemini-") {
        return Value::Null;
    }
    if runtime_model.starts_with("gemini-3.5-flash") || runtime_model == "gemini-3-flash-agent" {
        if effort == Effort::Off {
            return json!({ "includeThoughts": false, "thinkingBudget": 0 });
        }
        let thinking_budget = match effort {
            Effort::High => 10_000,
            Effort::Medium => 4_000,
            _ => 1_000,
        };
        return json!({ "includeThoughts": true, "thinkingBudget": thinking_budget });
    }
    if runtime_model.starts_with("gemini-3.1-pro") || runtime_model == "gemini-pro-agent" {
        if effort == Effort::Off {
            return json!({ "includeThoughts": false, "thinkingBudget": 0 });
        }
        let thinking_budget = match effort {
            Effort::High => 10_001,
            _ => 1_001,
        };
        return json!({ "includeThoughts": true, "thinkingBudget": thinking_budget });
    }
    if effort == Effort::Off {
        return json!({ "includeThoughts": false, "thinkingBudget": 0 });
    }
    let thinking_budget = match effort {
        Effort::High => -1,
        Effort::Medium => 4_000,
        _ => 1_000,
    };
    json!({ "includeThoughts": true, "thinkingBudget": thinking_budget })
}
