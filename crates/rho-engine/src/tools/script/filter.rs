use regex::Regex;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterResult {
    pub text: String,
    pub match_count: usize,
    pub total_lines: usize,
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("invalid filter regex {pattern:?}: {error}"))
}

pub fn filter_lines(content: &str, pattern: &str, context: usize) -> Result<FilterResult, String> {
    let regex = compile_pattern(pattern)?;
    if content.is_empty() {
        return Ok(FilterResult {
            text: format!("[No lines matched filter: {pattern:?}]"),
            match_count: 0,
            total_lines: 0,
        });
    }

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let mut match_indices = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            match_indices.push(i);
        }
    }

    if match_indices.is_empty() {
        return Ok(FilterResult {
            text: format!("[No lines matched filter: {pattern:?}]"),
            match_count: 0,
            total_lines,
        });
    }

    let match_count = match_indices.len();
    let match_set: HashSet<usize> = match_indices.iter().copied().collect();

    let mut intervals: Vec<(usize, usize)> = Vec::new();
    for &idx in &match_indices {
        let start = idx.saturating_sub(context);
        let end = (idx + context).min(total_lines.saturating_sub(1));
        if let Some(last) = intervals.last_mut() {
            if start <= last.1 + 1 {
                last.1 = last.1.max(end);
            } else {
                intervals.push((start, end));
            }
        } else {
            intervals.push((start, end));
        }
    }

    let mut output = String::new();
    for (span_idx, (start, end)) in intervals.iter().enumerate() {
        if span_idx > 0 {
            output.push_str("--\n");
        }
        for (line_idx, line) in lines.iter().enumerate().take(*end + 1).skip(*start) {
            let line_num = line_idx + 1;
            let sep = if match_set.contains(&line_idx) { ':' } else { '-' };
            output.push_str(&format!("{line_num}{sep}{line}\n"));
        }
    }

    Ok(FilterResult {
        text: output.trim_end().to_string(),
        match_count,
        total_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
fn first() {
    let a = 1;
    let b = 2;
}

fn second() {
    let target = 42;
    println!(\"{target}\");
}

fn third() {
    let c = 3;
}";

    #[test]
    fn test_filter_exact_match_zero_context() {
        let res = filter_lines(SAMPLE, "let target", 0).unwrap();
        assert_eq!(res.match_count, 1);
        assert_eq!(res.text, "7:    let target = 42;");
    }

    #[test]
    fn test_filter_with_context_expansion() {
        let res = filter_lines(SAMPLE, "let target", 1).unwrap();
        assert_eq!(res.match_count, 1);
        assert_eq!(
            res.text,
            "6-fn second() {\n7:    let target = 42;\n8-    println!(\"{target}\");"
        );
    }

    #[test]
    fn test_filter_overlapping_matches_merge() {
        let res = filter_lines(SAMPLE, "let a|let b", 1).unwrap();
        assert_eq!(res.match_count, 2);
        assert_eq!(res.text, "1-fn first() {\n2:    let a = 1;\n3:    let b = 2;\n4-}");
    }

    #[test]
    fn test_filter_disjoint_matches_separate_with_dashes() {
        let res = filter_lines(SAMPLE, "let a|let c", 0).unwrap();
        assert_eq!(res.match_count, 2);
        assert_eq!(res.text, "2:    let a = 1;\n--\n12:    let c = 3;");
    }

    #[test]
    fn test_filter_no_matches() {
        let res = filter_lines(SAMPLE, "nonexistent_term", 2).unwrap();
        assert_eq!(res.match_count, 0);
        assert!(res.text.contains("[No lines matched filter: \"nonexistent_term\"]"));
    }

    #[test]
    fn test_filter_smart_case() {
        let res = filter_lines("FooBar\nfoobar\nFOOBAR", "foobar", 0).unwrap();
        assert_eq!(res.match_count, 3);

        let res_case = filter_lines("FooBar\nfoobar\nFOOBAR", "FooBar", 0).unwrap();
        assert_eq!(res_case.match_count, 1);
    }

    #[test]
    fn test_filter_invalid_regex_returns_error() {
        let err = filter_lines(SAMPLE, "(unclosed", 2).unwrap_err();
        assert!(err.contains("invalid filter regex"));
    }
}
