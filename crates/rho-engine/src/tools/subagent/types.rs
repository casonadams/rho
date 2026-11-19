pub use rho_harness_core::args::subagent::{SubagentArgs, SubagentRole};

pub const DEFAULT_SUBAGENT_TURNS: usize = 100;
const MAX_TASK_NAME_CHARS: usize = 40;

#[must_use]
pub fn resolve_turns(requested: Option<usize>) -> usize {
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
pub fn task_name(raw: &str) -> Option<String> {
    let mut words = raw.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
    let mut name = lowered_first(words.next()?);
    name.extend(words.map(capitalized));
    Some(name.chars().take(MAX_TASK_NAME_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_default_to_100_with_no_upper_cap() {
        assert_eq!(resolve_turns(None), DEFAULT_SUBAGENT_TURNS);
        assert_eq!(resolve_turns(Some(0)), 1);
        assert_eq!(resolve_turns(Some(500)), 500);
    }

    #[test]
    fn task_names_are_camel_case_and_bounded() {
        let cases = [
            ("performanceAudit", Some("performanceAudit")),
            ("PerformanceAudit", Some("performanceAudit")),
            ("performance audit", Some("performanceAudit")),
            ("  retry-after_parsing v2 ", Some("retryAfterParsingV2")),
            ("HTTP retries", Some("httpRetries")),
            ("../etc/passwd", Some("etcPasswd")),
            ("  -- ", None),
            ("", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(task_name(raw).as_deref(), expected, "{raw}");
        }
        assert_eq!(
            task_name(&"a".repeat(100)).unwrap().chars().count(),
            MAX_TASK_NAME_CHARS
        );
    }
}
