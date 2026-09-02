/// Fuzzy subsequence matcher with scoring.
/// Lower score is better.
pub fn fuzzy_match(query: &str, target: &str) -> Option<i32> {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if query_lower.is_empty() {
        return Some(0);
    }
    if query_lower.len() > target_lower.len() {
        return None;
    }

    let mut query_chars = query_lower.chars().peekable();
    let mut score = 0i32;
    let mut last_match_index: Option<usize> = None;
    let mut consecutive_matches = 0i32;

    for (i, target_char) in target_lower.char_indices() {
        if let Some(&q_char) = query_chars.peek()
            && target_char == q_char
        {
            query_chars.next();

            let is_word_boundary = i == 0
                || target_lower[..i]
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_whitespace() || c == '-' || c == '_' || c == '/' || c == '.');

            if let Some(last_idx) = last_match_index {
                if last_idx + target_char.len_utf8() == i {
                    consecutive_matches += 1;
                    score -= consecutive_matches * 5;
                } else {
                    consecutive_matches = 0;
                    score += (i.saturating_sub(last_idx + 1) as i32) * 2;
                }
            }

            if is_word_boundary {
                score -= 10;
            }

            score += (i as i32) / 2;
            last_match_index = Some(i);
        }
    }

    if query_chars.peek().is_none() {
        if query_lower == target_lower {
            score -= 100;
        }
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact() {
        let score = fuzzy_match("model", "model");
        assert!(score.is_some());
        let exact_score = score.unwrap();

        let partial_score = fuzzy_match("mod", "model").unwrap();
        assert!(exact_score < partial_score);
    }

    #[test]
    fn test_fuzzy_match_prefix_and_subsequence() {
        assert!(fuzzy_match("clear", "clear").is_some());
        assert!(fuzzy_match("clr", "clear").is_some());
        assert!(fuzzy_match("ex", "export").is_some());
        assert!(fuzzy_match("exp", "export").is_some());
        assert!(fuzzy_match("xyz", "export").is_none());
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        assert!(fuzzy_match("MODEL", "model").is_some());
        assert!(fuzzy_match("model", "MODEL").is_some());
    }
}
