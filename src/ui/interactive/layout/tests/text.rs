use crate::ui::interactive::layout::{truncate_to_visual_lines, wrap_words_to_width};

#[test]
fn truncate_to_visual_lines_preserves_short_content() {
    let text = "line1\nline2\nline3";
    let res = truncate_to_visual_lines(text, 5, 40);
    assert_eq!(res.visual_lines, ["line1", "line2", "line3"]);
    assert_eq!(res.skipped_count, 0);
}

#[test]
fn truncate_to_visual_lines_skips_earlier_lines_when_exceeding_limit() {
    let text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
    let res = truncate_to_visual_lines(text, 5, 40);
    assert_eq!(res.visual_lines, ["line3", "line4", "line5", "line6", "line7"]);
    assert_eq!(res.skipped_count, 2);
}

#[test]
fn wrap_words_to_width_breaks_on_word_boundaries() {
    let text = "alpha beta gamma delta epsilon";
    let wrapped = wrap_words_to_width(text, 16);
    assert_eq!(wrapped, vec!["alpha beta gamma", "delta epsilon"]);
}

#[test]
fn wrap_words_to_width_splits_oversized_words() {
    let text = "short supercalifragilisticexpialidocious end";
    let wrapped = wrap_words_to_width(text, 10);
    assert_eq!(
        wrapped,
        vec!["short", "supercalif", "ragilistic", "expialidoc", "ious end",]
    );
}

#[test]
fn wrap_words_to_width_handles_empty_and_whitespace_lines() {
    let text = "first\n\n   \nsecond";
    let wrapped = wrap_words_to_width(text, 20);
    assert_eq!(wrapped, vec!["first", "", "", "second"]);
}
