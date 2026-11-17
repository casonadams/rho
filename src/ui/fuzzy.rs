use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// Case-insensitive fuzzy score of `query` within `target`: `None` when the
/// query characters do not appear in order. Higher is better.
pub(crate) fn fuzzy_score(query: &str, target: &str) -> Option<i64> {
    SkimMatcherV2::default().ignore_case().fuzzy_match(target, query)
}

#[cfg(test)]
mod tests {
    use super::fuzzy_score;

    #[test]
    fn matches_ordered_subsequences_case_insensitively() {
        let cases = [
            ("", "anything", true),
            ("cl", "claude", true),
            ("CL", "claude", true),
            ("c3o", "claude-3-opus", true),
            ("xyz", "claude", false),
            ("longerthan", "short", false),
        ];
        for (query, target, matches) in cases {
            assert_eq!(fuzzy_score(query, target).is_some(), matches, "{query:?} in {target:?}");
        }
    }

    #[test]
    fn word_boundary_and_prefix_matches_rank_higher() {
        assert!(fuzzy_score("sonnet", "claude-3-5-sonnet") > fuzzy_score("onnet", "claude-3-5-sonnet"));
        assert!(fuzzy_score("m", "model") > fuzzy_score("m", "compact"));
    }
}
