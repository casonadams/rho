use crate::ui::interactive::layout::{truncate_to_visual_lines, wrap_to_width};
use test_case::test_case;

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

#[test_case("alpha beta gamma delta epsilon", 16, &["alpha beta gamma", "delta epsilon"]; "word boundaries")]
#[test_case(
    "short supercalifragilisticexpialidocious end",
    10,
    &["short", "supercalif", "ragilistic", "expialidoc", "ious end"];
    "oversized words split"
)]
#[test_case("first\n\n   \nsecond", 20, &["first", "", "", "second"]; "blank lines kept")]
#[test_case(
    "\x1b[32malpha beta gamma delta\x1b[0m",
    12,
    &["\x1b[32malpha beta\x1b[0m", "\x1b[32mgamma delta\x1b[0m"];
    "sgr reopened per line"
)]
#[test_case("  alpha beta gamma", 10, &["  alpha", "beta gamma"]; "first line indentation kept")]
#[test_case("a\n  b", 10, &["a", "  b"]; "source line indentation kept")]
#[test_case("alpha   beta", 20, &["alpha   beta"]; "inner spaces kept")]
#[test_case(
    "\r00:01 +0: loading\r\n\r00:02 +1: passed\r\n",
    40,
    &["00:01 +0: loading", "00:02 +1: passed", ""];
    "carriage returns stripped"
)]
#[test_case("\x1b[2K\x1b[31mred", 80, &["\x1b[2K\x1b[31mred"]; "non sgr escapes kept")]
fn wrap_to_width_breaks_lines_at_the_column_width(text: &str, width: usize, expected: &[&str]) {
    assert_eq!(wrap_to_width(text, width), expected);
}

#[test]
fn wrap_keeps_visible_text_after_non_sgr_escape_sequences() {
    let wrapped = wrap_to_width("\x1b[2Kkeep this text", 80);
    assert_eq!(crate::ui::block::strip_ansi(&wrapped.join("\n")), "keep this text");
}
