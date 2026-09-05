use super::token::{DiffToken, compute_token_diff, tokenize};
use super::word::render_single_line_word_diff;
use crate::ui::theme::Theme;

#[test]
fn tokenize_handles_multibyte_unicode_without_panicking() {
    let text = "### Slice 1: File Transclusion — **2 pts**";
    let tokens = tokenize(text);
    assert!(!tokens.is_empty());
    let joined = tokens.concat();
    assert_eq!(joined, text);
}

#[test]
fn tokenize_handles_emojis_and_cjk_characters() {
    let text = "🦀 emoji and 中文 text — with dashes";
    let tokens = tokenize(text);
    let joined = tokens.concat();
    assert_eq!(joined, text);
}

#[test]
fn tokenize_handles_empty_and_single_char() {
    assert_eq!(tokenize(""), Vec::<&str>::new());
    assert_eq!(tokenize("a"), vec!["a"]);
    assert_eq!(tokenize("—"), vec!["—"]);
    assert_eq!(tokenize("🦀"), vec!["🦀"]);
}

#[test]
fn single_line_word_diff_with_unicode_does_not_panic() {
    let theme = Theme::default();
    let old_line = "### Slice 1: File Transclusion — **2 pts** (Draft)";
    let new_line = "### Slice 1: File Transclusion — **2 pts** (Complete)";

    let (removed, added) = render_single_line_word_diff(old_line, new_line, &theme);
    assert!(removed.contains("Draft"));
    assert!(added.contains("Complete"));
    assert!(removed.contains("—"));
    assert!(added.contains("—"));
}

#[test]
fn compute_token_diff_matches_identical_tokens() {
    let old_tokens = vec!["hello", " ", "world"];
    let new_tokens = vec!["hello", " ", "world"];
    let diff = compute_token_diff(&old_tokens, &new_tokens);
    assert_eq!(
        diff,
        vec![DiffToken::Same("hello"), DiffToken::Same(" "), DiffToken::Same("world"),]
    );
}

#[test]
fn compute_token_diff_handles_add_and_remove() {
    let old_tokens = vec!["old"];
    let new_tokens = vec!["new"];
    let diff = compute_token_diff(&old_tokens, &new_tokens);
    assert_eq!(diff, vec![DiffToken::Removed("old"), DiffToken::Added("new")]);
}
