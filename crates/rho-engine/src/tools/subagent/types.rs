pub use rho_harness_core::args::subagent::{SubagentArgs, SubagentRole};

pub(crate) const DEFAULT_SUBAGENT_TURNS: usize = 100;
const MAX_TASK_NAME_CHARS: usize = 40;

#[must_use]
pub(crate) fn resolve_turns(requested: Option<usize>) -> usize {
    requested.unwrap_or(DEFAULT_SUBAGENT_TURNS).max(1)
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn lowered_first(word: &str) -> String {
    if word.chars().all(|c| !c.is_lowercase()) {
        return word.to_lowercase();
    }
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The camelCase task name a subagent is labelled with, e.g. `performance audit` -> `performanceAudit`.
#[must_use]
pub(crate) fn task_name(raw: &str) -> Option<String> {
    let mut words = raw.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
    let mut name = lowered_first(words.next()?);
    name.extend(words.map(capitalized));
    Some(name.chars().take(MAX_TASK_NAME_CHARS).collect())
}

/// The label a named subagent run is shown under, e.g. `subagent performanceAudit`.
#[must_use]
pub(super) fn subagent_label(name: &str) -> String {
    format!("subagent {name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn turns_default_to_100_with_no_upper_cap() {
        assert_eq!(resolve_turns(None), DEFAULT_SUBAGENT_TURNS);
        assert_eq!(resolve_turns(Some(0)), 1);
        assert_eq!(resolve_turns(Some(500)), 500);
    }

    #[test_case("performanceAudit", Some("performanceAudit"); "camel case")]
    #[test_case("PerformanceAudit", Some("performanceAudit"); "pascal case")]
    #[test_case("performance audit", Some("performanceAudit"); "spaced words")]
    #[test_case("  retry-after_parsing v2 ", Some("retryAfterParsingV2"); "mixed separators")]
    #[test_case("HTTP retries", Some("httpRetries"); "acronym")]
    #[test_case("../etc/passwd", Some("etcPasswd"); "path characters")]
    #[test_case("  -- ", None; "separators only")]
    #[test_case("", None; "empty")]
    fn task_names_are_camel_case(raw: &str, expected: Option<&str>) {
        assert_eq!(task_name(raw).as_deref(), expected);
    }

    #[test]
    fn task_names_are_bounded() {
        assert_eq!(
            task_name(&"a".repeat(100)).unwrap().chars().count(),
            MAX_TASK_NAME_CHARS
        );
    }
}
