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
    use test_case::test_case;

    #[test_case("", "anything", true; "empty query")]
    #[test_case("cl", "claude", true; "prefix")]
    #[test_case("CL", "claude", true; "uppercase query")]
    #[test_case("c3o", "claude-3-opus", true; "subsequence")]
    #[test_case("xyz", "claude", false; "absent chars")]
    #[test_case("longerthan", "short", false; "query longer than target")]
    fn matches_ordered_subsequences_case_insensitively(query: &str, target: &str, matches: bool) {
        assert_eq!(fuzzy_score(query, target).is_some(), matches);
    }

    #[test]
    fn word_boundary_and_prefix_matches_rank_higher() {
        assert!(fuzzy_score("sonnet", "claude-3-5-sonnet") > fuzzy_score("onnet", "claude-3-5-sonnet"));
        assert!(fuzzy_score("m", "model") > fuzzy_score("m", "compact"));
    }
}
