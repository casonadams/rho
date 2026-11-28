use super::normalize::{has_whitespace_relaxed_match, normalize_line_endings, truncate_snippet};
use rho_harness_core::args::EditReplacement;
use std::ops::Range;

/// Why an edit matched nothing; a missing anchor is described only on request, since its
/// whitespace hint rescans the whole file.
pub(crate) enum EditMiss<'a> {
    NotFound {
        field: &'static str,
        searched: &'a str,
        text: &'a str,
    },
    Rejected(String),
}

impl EditMiss<'_> {
    pub(crate) fn describe(self) -> String {
        match self {
            Self::NotFound { field, searched, text } => not_found(field, searched, text),
            Self::Rejected(message) => message,
        }
    }
}

/// Byte ranges an edit replaces in `content`, in ascending order.
pub(crate) fn locate_edit(
    content: &str,
    edit: &EditReplacement,
    line_ending: &str,
) -> Result<Vec<Range<usize>>, String> {
    match_edit(content, edit, line_ending).map_err(EditMiss::describe)
}

/// Like `locate_edit`, leaving a miss undescribed.
pub(crate) fn match_edit<'a>(
    content: &'a str,
    edit: &'a EditReplacement,
    line_ending: &str,
) -> Result<Vec<Range<usize>>, EditMiss<'a>> {
    let rejected = |message: &str| Err(EditMiss::Rejected(message.to_string()));
    match (&edit.start_text, &edit.end_text) {
        (None, None) => locate_old_text(content, edit, line_ending),
        (Some(start), Some(end)) if edit.old_text.is_empty() && edit.replace_all != Some(true) => {
            locate_region(content, start, end, line_ending).map(|range| vec![range])
        }
        (Some(_), Some(_)) => rejected("startText/endText cannot be combined with oldText or replaceAll"),
        _ => rejected("startText and endText must be given together"),
    }
}

fn locate_old_text<'a>(
    content: &'a str,
    edit: &'a EditReplacement,
    line_ending: &str,
) -> Result<Vec<Range<usize>>, EditMiss<'a>> {
    let old = normalize_line_endings(&edit.old_text, line_ending);
    if old.is_empty() {
        return Err(EditMiss::Rejected(
            "oldText must not be empty (or use startText/endText)".to_string(),
        ));
    }
    let ranges: Vec<Range<usize>> = content
        .match_indices(old.as_ref())
        .map(|(at, _)| at..at + old.len())
        .collect();
    match ranges.len() {
        0 => Err(EditMiss::NotFound {
            field: "oldText",
            searched: content,
            text: &edit.old_text,
        }),
        1 => Ok(ranges),
        _ if edit.replace_all == Some(true) => Ok(ranges),
        count => Err(EditMiss::Rejected(format!(
            "oldText found {count} times in file (must be unique):\n{}\n\nNote: Provide more surrounding context lines in oldText to disambiguate the match, or set replaceAll to change every occurrence.",
            truncate_snippet(&edit.old_text, 120)
        ))),
    }
}

fn locate_region<'a>(
    content: &'a str,
    start: &'a str,
    end: &'a str,
    line_ending: &str,
) -> Result<Range<usize>, EditMiss<'a>> {
    let start_norm = normalize_line_endings(start, line_ending);
    let end_norm = normalize_line_endings(end, line_ending);
    if start_norm.is_empty() || end_norm.is_empty() {
        return Err(EditMiss::Rejected(
            "startText and endText must not be empty".to_string(),
        ));
    }
    let mut starts = content.match_indices(start_norm.as_ref()).map(|(at, _)| at);
    let begin = match (starts.next(), starts.next()) {
        (None, _) => {
            return Err(EditMiss::NotFound {
                field: "startText",
                searched: content,
                text: start,
            });
        }
        (Some(_), Some(_)) => {
            return Err(EditMiss::Rejected(format!(
                "startText found {} times in file (must be unique):\n{}",
                2 + starts.count(),
                truncate_snippet(start, 120)
            )));
        }
        (Some(at), None) => at,
    };
    let after_start = begin + start_norm.len();
    let Some(offset) = content[after_start..].find(end_norm.as_ref()) else {
        return Err(EditMiss::NotFound {
            field: "endText (after startText)",
            searched: &content[after_start..],
            text: end,
        });
    };
    Ok(begin..after_start + offset + end_norm.len())
}

fn not_found(field: &str, content: &str, text: &str) -> String {
    let hint = if has_whitespace_relaxed_match(content, text) {
        "\n\nNote: A matching block with different whitespace or indentation was found. Verify exact indentation and line breaks."
    } else {
        ""
    };
    format!(
        "{field} not found in file (exact match required):\n{}{hint}",
        truncate_snippet(text, 120)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, start: Option<&str>, end: Option<&str>, all: bool) -> EditReplacement {
        EditReplacement {
            old_text: old.to_string(),
            new_text: String::new(),
            start_text: start.map(Into::into),
            end_text: end.map(Into::into),
            replace_all: all.then_some(true),
        }
    }

    #[test]
    fn locates_unique_text_all_occurrences_and_regions() {
        let content = "a foo b foo c\nfn x() {\n  body\n}\nend";
        let unique = locate_edit(content, &edit("b foo", None, None, false), "\n").unwrap();
        assert_eq!(unique.len(), 1);
        assert_eq!(&content[unique[0].clone()], "b foo");
        assert_eq!(
            locate_edit(content, &edit("foo", None, None, true), "\n"),
            Ok(vec![2..5, 8..11])
        );
        let region = locate_edit(content, &edit("", Some("fn x() {"), Some("}"), false), "\n").unwrap();
        assert_eq!(&content[region[0].clone()], "fn x() {\n  body\n}");
    }

    #[test]
    fn rejects_ambiguous_missing_and_mixed_forms() {
        let content = "foo foo {}";
        let cases = [
            (edit("foo", None, None, false), "found 2 times"),
            (edit("bar", None, None, false), "oldText not found"),
            (edit("", None, None, false), "must not be empty"),
            (edit("", Some("foo"), Some("}"), false), "startText found 2 times"),
            (
                edit("", Some("{"), Some("zzz"), false),
                "endText (after startText) not found",
            ),
            (edit("foo", Some("{"), Some("}"), false), "cannot be combined"),
            (edit("", Some("{"), Some("}"), true), "cannot be combined"),
            (edit("", Some("{"), None, false), "must be given together"),
        ];
        for (case, expected) in cases {
            let err = locate_edit(content, &case, "\n").unwrap_err();
            assert!(err.contains(expected), "{case:?}: {err}");
        }
    }

    #[test]
    fn end_anchor_must_follow_the_start_anchor() {
        let content = "}\nstart\nmiddle\n}";
        let range = locate_edit(content, &edit("", Some("start"), Some("}"), false), "\n").unwrap();
        assert_eq!(&content[range[0].clone()], "start\nmiddle\n}");
    }
}
