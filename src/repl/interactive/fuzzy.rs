struct MatchScorer {
    score: i32,
    last_match_index: Option<usize>,
    consecutive_matches: i32,
}

impl MatchScorer {
    fn new() -> Self {
        Self {
            score: 0,
            last_match_index: None,
            consecutive_matches: 0,
        }
    }

    fn record_match(&mut self, (idx, char_len): (usize, usize), is_word_boundary: bool) {
        if let Some(last_idx) = self.last_match_index {
            if last_idx + char_len == idx {
                self.consecutive_matches += 1;
                self.score -= self.consecutive_matches * 5;
            } else {
                self.consecutive_matches = 0;
                self.score += (idx.saturating_sub(last_idx + 1) as i32) * 2;
            }
        }
        if is_word_boundary {
            self.score -= 10;
        }
        self.score += (idx as i32) / 2;
        self.last_match_index = Some(idx);
    }
}

fn is_boundary(target_lower: &str, idx: usize) -> bool {
    idx == 0
        || target_lower[..idx]
            .chars()
            .last()
            .is_some_and(|c| c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.'))
}

fn score_matching_chars(query: &str, target: &str) -> Option<i32> {
    let mut query_chars = query.chars().peekable();
    let mut scorer = MatchScorer::new();

    for (i, target_char) in target.char_indices() {
        if query_chars.peek() == Some(&target_char) {
            query_chars.next();
            scorer.record_match((i, target_char.len_utf8()), is_boundary(target, i));
        }
    }

    if query_chars.peek().is_some() {
        return None;
    }
    if query == target {
        scorer.score -= 100;
    }
    Some(scorer.score)
}

/// Fuzzy subsequence matcher with scoring.
/// Lower score is better.
pub fn fuzzy_match(query: &str, target: &str) -> Option<i32> {
    let (query_lower, target_lower) = (query.to_lowercase(), target.to_lowercase());

    if query_lower.is_empty() {
        return Some(0);
    }
    if query_lower.len() > target_lower.len() {
        return None;
    }

    score_matching_chars(&query_lower, &target_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_scores_best() {
        assert!(fuzzy_match("claude", "claude").unwrap() < fuzzy_match("claude", "claude-3-opus").unwrap());
    }

    #[test]
    fn prefix_match_scores_well() {
        assert!(fuzzy_match("cl", "claude").is_some());
    }

    #[test]
    fn word_boundary_match_scores_better() {
        let boundary = fuzzy_match("sonnet", "claude-3-5-sonnet").unwrap();
        let mid_word = fuzzy_match("onnet", "claude-3-5-sonnet").unwrap();
        assert!(boundary < mid_word);
    }

    #[test]
    fn empty_query_matches_all() {
        assert_eq!(fuzzy_match("", "anything"), Some(0));
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert!(fuzzy_match("xyz", "claude").is_none());
    }

    #[test]
    fn longer_query_than_target_returns_none() {
        assert!(fuzzy_match("longerthan", "short").is_none());
    }
}
