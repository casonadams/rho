//! `.guard.yml` configuration loader, production/preview environment detection,
//! allowlist regex matching, and per-model calibrated risk thresholds.
//!
//! Mirrors `guard@casonadams-plugins` so the same `.guard.yml` files work seamlessly
//! in both `omp` and `rho`.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const DEFAULT_PRODUCTION_MARKERS: &[&str] = &["prod", "prd", "production", "live", "kube-system"];
pub const DEFAULT_PREVIEW_MARKERS: &[&str] = &["pr-", "preview", "review-app"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardProductionConfig {
    pub namespaces: Vec<String>,
    pub projects: Vec<String>,
    pub clusters: Vec<String>,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardPreviewConfig {
    pub namespaces: Vec<String>,
    pub domains: Vec<String>,
    pub clusters: Vec<String>,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardTelemetryConfig {
    pub enabled: Option<bool>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardThresholdValues {
    pub p1_prod_impact: Option<f64>,
    pub p2_malicious: Option<f64>,
    pub p2_malicious_preview: Option<f64>,
    pub allow_ambiguous_in_preview: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardThresholdsConfig {
    pub values: GuardThresholdValues,
    pub by_model: BTreeMap<String, GuardThresholdValues>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardConfig {
    pub production: GuardProductionConfig,
    pub preview: GuardPreviewConfig,
    pub allowlist: Vec<String>,
    pub telemetry: GuardTelemetryConfig,
    pub thresholds: GuardThresholdsConfig,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedGuardThresholds {
    pub p1_prod_impact: f64,
    pub p2_malicious: f64,
    pub p2_malicious_preview: f64,
    pub allow_ambiguous_in_preview: bool,
}

impl Default for ResolvedGuardThresholds {
    fn default() -> Self {
        Self {
            p1_prod_impact: 0.05,
            p2_malicious: 0.10,
            p2_malicious_preview: 0.40,
            allow_ambiguous_in_preview: true,
        }
    }
}

/// Locates the first existing guard config file in precedence order:
/// project `.guard.yml`/`.guard.yaml`, `~/.config/rho/guard.yml`,
/// `~/.config/omp-guard/config.yml`, then `~/.guard.yml`.
pub fn find_guard_config_file(cwd: Option<&Path>, home_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = cwd {
        candidates.push(dir.join(".guard.yml"));
        candidates.push(dir.join(".guard.yaml"));
    }
    if let Some(home) = home_dir {
        candidates.push(home.join(".config").join("rho").join("guard.yml"));
        candidates.push(home.join(".config").join("rho").join("guard.yaml"));
        candidates.push(home.join(".config").join("omp-guard").join("config.yml"));
        candidates.push(home.join(".config").join("omp-guard").join("config.yaml"));
        candidates.push(home.join(".guard.yml"));
        candidates.push(home.join(".guard.yaml"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn load_guard_config(cwd: Option<&Path>, home_dir: Option<&Path>) -> GuardConfig {
    let Some(path) = find_guard_config_file(cwd, home_dir) else {
        return GuardConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return GuardConfig::default();
    };
    parse_guard_yaml(&raw)
}

#[derive(Default)]
struct ParseCursor {
    section: String,
    subsection: String,
    model_key: Option<String>,
}

fn unquote_yaml(raw: &str) -> String {
    let s = strip_inline_comment(raw).trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        let inner = &s[1..s.len() - 1];
        if s.starts_with('"') {
            inner.replace("\\\\", "\\").replace("\\\"", "\"")
        } else {
            inner.replace("''", "'")
        }
    } else {
        s.to_string()
    }
}

fn strip_inline_comment(raw: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double && (idx == 0 || raw[..idx].ends_with(char::is_whitespace)) => {
                return &raw[..idx];
            }
            _ => {}
        }
    }
    raw
}

fn apply_threshold_field(target: &mut GuardThresholdValues, key: &str, val: &str) {
    match key {
        "p1ProdImpact" | "p1_prod_impact" => target.p1_prod_impact = val.parse::<f64>().ok(),
        "p2Malicious" | "p2_malicious" => target.p2_malicious = val.parse::<f64>().ok(),
        "p2MaliciousPreview" | "p2_malicious_preview" => target.p2_malicious_preview = val.parse::<f64>().ok(),
        "allowAmbiguousInPreview" | "allow_ambiguous_in_preview" => {
            target.allow_ambiguous_in_preview = val.parse::<bool>().ok();
        }
        _ => {}
    }
}

fn push_list_item(cfg: &mut GuardConfig, cursor: &ParseCursor, item: String) {
    if item.is_empty() {
        return;
    }
    match (cursor.section.as_str(), cursor.subsection.as_str()) {
        ("allowlist", _) => cfg.allowlist.push(item),
        ("production", "namespaces") => cfg.production.namespaces.push(item),
        ("production", "projects") => cfg.production.projects.push(item),
        ("production", "clusters") => cfg.production.clusters.push(item),
        ("production", "markers") => cfg.production.markers.push(item),
        ("preview", "namespaces") => cfg.preview.namespaces.push(item),
        ("preview", "domains") => cfg.preview.domains.push(item),
        ("preview", "clusters") => cfg.preview.clusters.push(item),
        ("preview", "markers") => cfg.preview.markers.push(item),
        _ => {}
    }
}

fn apply_key_value(cfg: &mut GuardConfig, cursor: &mut ParseCursor, indent: usize, key: &str, val: &str) {
    if indent == 0 {
        cursor.section = key.to_string();
        cursor.subsection.clear();
        cursor.model_key = None;
        return;
    }
    if cursor.section == "thresholds" {
        apply_threshold_line(cfg, cursor, indent, key, val);
        return;
    }
    if cursor.section == "telemetry" {
        match key {
            "enabled" => cfg.telemetry.enabled = val.parse::<bool>().ok(),
            "path" if !val.is_empty() => cfg.telemetry.path = Some(val.to_string()),
            _ => {}
        }
        return;
    }
    if val.is_empty() {
        cursor.subsection = key.to_string();
    }
}

fn apply_threshold_line(cfg: &mut GuardConfig, cursor: &mut ParseCursor, indent: usize, key: &str, val: &str) {
    if key == "byModel" || key == "by_model" {
        cursor.subsection = "byModel".to_string();
        cursor.model_key = None;
        return;
    }
    if cursor.subsection == "byModel" {
        if val.is_empty() && indent <= 4 {
            cursor.model_key = Some(key.to_string());
        } else if let Some(model) = cursor.model_key.as_deref() {
            let entry = cfg.thresholds.by_model.entry(model.to_string()).or_default();
            apply_threshold_field(entry, key, val);
        }
        return;
    }
    apply_threshold_field(&mut cfg.thresholds.values, key, val);
}

/// Parses `.guard.yml` / `config.yml` YAML into [`GuardConfig`].
pub fn parse_guard_yaml(raw: &str) -> GuardConfig {
    let mut cfg = GuardConfig::default();
    let mut cursor = ParseCursor::default();
    for line in raw.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let trimmed = strip_inline_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            push_list_item(&mut cfg, &cursor, unquote_yaml(rest));
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = unquote_yaml(k);
            let val = unquote_yaml(v);
            apply_key_value(&mut cfg, &mut cursor, indent, &key, &val);
        }
    }
    cfg
}

fn clean_markers(sources: &[&[String]], defaults: &[&str]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for d in defaults {
        if seen.insert((*d).to_string()) {
            out.push((*d).to_string());
        }
    }
    for list in sources {
        for item in *list {
            let cleaned = item.replace(['*', '^', '$'], "").trim().to_ascii_lowercase();
            if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
                out.push(cleaned);
            }
        }
    }
    out
}

pub fn resolve_production_markers(config: &GuardConfig) -> Vec<String> {
    clean_markers(
        &[
            &config.production.markers,
            &config.production.namespaces,
            &config.production.projects,
            &config.production.clusters,
        ],
        DEFAULT_PRODUCTION_MARKERS,
    )
}

pub fn resolve_preview_markers(config: &GuardConfig) -> Vec<String> {
    clean_markers(
        &[
            &config.preview.markers,
            &config.preview.namespaces,
            &config.preview.domains,
            &config.preview.clusters,
        ],
        DEFAULT_PREVIEW_MARKERS,
    )
}

fn matches_wildcard(value: &str, pattern: &str) -> bool {
    let escaped = regex::escape(pattern).replace("\\*", ".*");
    Regex::new(&format!("(?i)^{escaped}$")).is_ok_and(|re| re.is_match(value))
}

fn capture_flag_values(command: &str, pattern: &str) -> Vec<String> {
    let Ok(re) = Regex::new(pattern) else {
        return Vec::new();
    };
    re.captures_iter(command)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_ascii_lowercase()))
        .collect()
}

fn extract_hosts(command: &str) -> Vec<String> {
    capture_flag_values(command, r"(?i)https?://([^\s/'\x22]+)")
}

fn any_flag_matches(values: &[String], markers: &[String], patterns: &[String], substring: bool) -> bool {
    values.iter().any(|v| {
        markers.iter().any(|m| {
            if substring {
                v.contains(m)
            } else {
                v == m || v.starts_with(m)
            }
        }) || patterns.iter().any(|p| matches_wildcard(v, p))
    })
}

pub fn is_command_targeting_production(command: &str, config: &GuardConfig) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }
    let markers = resolve_production_markers(config);
    let ns_vals = capture_flag_values(trimmed, r#"(?i)(?:-n\s+|--namespace(?:=|\s+))([^\s'"]+)"#);
    if any_flag_matches(&ns_vals, &markers, &config.production.namespaces, false) {
        return true;
    }
    let ctx_vals = capture_flag_values(trimmed, r#"(?i)--context(?:=|\s+)([^\s'"]+)"#);
    if any_flag_matches(&ctx_vals, &markers, &config.production.clusters, true) {
        return true;
    }
    let proj_vals = capture_flag_values(trimmed, r#"(?i)--project(?:=|\s+)([^\s'"]+)"#);
    if any_flag_matches(&proj_vals, &markers, &config.production.projects, true) {
        return true;
    }
    let host_re = Regex::new(r"(?i)\b(prod|prd|production)\b").expect("valid host regex");
    extract_hosts(trimmed).iter().any(|h| host_re.is_match(h))
}

fn matches_preview_ns(command: &str, config: &GuardConfig) -> bool {
    let ns_vals = capture_flag_values(command, r#"(?i)(?:-n\s+|--namespace(?:=|\s+))([^\s'"]+)"#);
    ns_vals.iter().any(|val| {
        val.starts_with("pr-")
            || val.starts_with("preview-")
            || config.preview.namespaces.iter().any(|p| matches_wildcard(val, p))
            || config
                .preview
                .markers
                .iter()
                .any(|m| val.contains(&m.to_ascii_lowercase()))
    })
}

fn matches_preview_domain(command: &str, config: &GuardConfig) -> bool {
    extract_hosts(command).iter().any(|host| {
        host.starts_with("pr-")
            || host.contains(".dev.previ.dev")
            || host.contains("preview")
            || config.preview.domains.iter().any(|p| matches_wildcard(host, p))
    })
}

fn matches_preview_context(command: &str, config: &GuardConfig) -> bool {
    let ctx_vals = capture_flag_values(command, r#"(?i)--context(?:=|\s+)([^\s'"]+)"#);
    ctx_vals.iter().any(|val| {
        val.contains("preview")
            || val.starts_with("pr-")
            || config.preview.clusters.iter().any(|p| matches_wildcard(val, p))
    })
}

pub fn is_command_targeting_preview(command: &str, config: &GuardConfig) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() || is_command_targeting_production(trimmed, config) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    matches_preview_ns(trimmed, config)
        || matches_preview_domain(trimmed, config)
        || matches_preview_context(trimmed, config)
        || config
            .preview
            .markers
            .iter()
            .any(|m| !m.is_empty() && lower.contains(&m.to_ascii_lowercase()))
}

pub fn is_allowlisted_command(command: &str, config: &GuardConfig) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() || config.allowlist.is_empty() {
        return false;
    }
    config.allowlist.iter().any(|pattern| {
        let p = pattern.trim();
        !p.is_empty() && Regex::new(&format!("(?i){p}")).is_ok_and(|re| re.is_match(trimmed))
    })
}

fn clamp_unit(value: Option<f64>, fallback: f64) -> f64 {
    match value {
        Some(v) if v.is_finite() && (0.0..=1.0).contains(&v) => v,
        _ => fallback,
    }
}

fn apply_threshold_overrides(base: ResolvedGuardThresholds, ov: &GuardThresholdValues) -> ResolvedGuardThresholds {
    ResolvedGuardThresholds {
        p1_prod_impact: clamp_unit(ov.p1_prod_impact, base.p1_prod_impact),
        p2_malicious: clamp_unit(ov.p2_malicious, base.p2_malicious),
        p2_malicious_preview: clamp_unit(ov.p2_malicious_preview, base.p2_malicious_preview),
        allow_ambiguous_in_preview: ov.allow_ambiguous_in_preview.unwrap_or(base.allow_ambiguous_in_preview),
    }
}

pub fn resolve_thresholds(config: &GuardConfig, model_selector: Option<&str>) -> ResolvedGuardThresholds {
    let top = apply_threshold_overrides(ResolvedGuardThresholds::default(), &config.thresholds.values);
    let Some(selector) = model_selector else {
        return top;
    };
    let bare = selector.split_once('/').map_or(selector, |(_, m)| m);
    let override_val = config
        .thresholds
        .by_model
        .get(selector)
        .or_else(|| config.thresholds.by_model.get(bare));
    override_val.map_or(top, |ov| apply_threshold_overrides(top, ov))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"
allowlist:
  - "(^|\\s|/)ripwire(\\s|$)"

production:
  namespaces:
    - "prod*"
  projects:
    - "*prod*"

preview:
  namespaces:
    - "pr-*"
    - "preview-*"
  domains:
    - "*.dev.previ.dev"

thresholds:
  p1ProdImpact: 0.15
  p2Malicious: 0.20
  p2MaliciousPreview: 0.45
  allowAmbiguousInPreview: true
  byModel:
    gemini/gemini-decision-1:
      p1ProdImpact: 0.12
"#;

    #[test]
    fn parses_omp_guard_yaml_and_resolves_thresholds() {
        let cfg = parse_guard_yaml(SAMPLE_YAML);
        assert_eq!(cfg.allowlist, vec![r"(^|\s|/)ripwire(\s|$)"]);
        assert_eq!(cfg.preview.namespaces, vec!["pr-*", "preview-*"]);
        assert_eq!(cfg.preview.domains, vec!["*.dev.previ.dev"]);

        let base = resolve_thresholds(&cfg, None);
        assert_eq!(base.p1_prod_impact, 0.15);
        assert_eq!(base.p2_malicious, 0.20);
        assert_eq!(base.p2_malicious_preview, 0.45);
        assert!(base.allow_ambiguous_in_preview);

        let per_model = resolve_thresholds(&cfg, Some("gemini/gemini-decision-1"));
        assert_eq!(per_model.p1_prod_impact, 0.12);
        assert_eq!(per_model.p2_malicious, 0.20);
    }

    #[test]
    fn detects_allowlist_preview_and_production_invariance() {
        let cfg = parse_guard_yaml(SAMPLE_YAML);
        assert!(is_allowlisted_command("ripwire . --report", &cfg));
        assert!(!is_allowlisted_command("cargo test", &cfg));

        assert!(is_command_targeting_preview(
            "kubectl get pods --namespace=pr-499",
            &cfg
        ));
        assert!(is_command_targeting_preview(
            "curl https://pr-499.dev.previ.dev/health",
            &cfg
        ));
        assert!(is_command_targeting_production(
            "kubectl delete pod x --namespace=prod-api",
            &cfg
        ));
        assert!(!is_command_targeting_preview(
            "kubectl -n pr-499 get pods && kubectl -n prod-api delete pod x",
            &cfg
        ));
    }
}
