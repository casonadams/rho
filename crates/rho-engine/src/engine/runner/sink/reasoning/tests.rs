use super::split_reasoning_chunk;

#[test]
fn plain_text_has_no_trailing_newlines() {
    assert_eq!(split_reasoning_chunk("thinking"), ("thinking", 0));
}

#[test]
fn trailing_space_without_newline_is_preserved_for_word_streaming() {
    assert_eq!(split_reasoning_chunk("thinking "), ("thinking ", 0));
    assert_eq!(split_reasoning_chunk("  "), ("  ", 0));
}

#[test]
fn trailing_newlines_are_counted_and_stripped() {
    assert_eq!(split_reasoning_chunk("step 1\n"), ("step 1", 1));
    assert_eq!(split_reasoning_chunk("step 1\n\n\n"), ("step 1", 3));
    assert_eq!(split_reasoning_chunk("step 1  \n\n"), ("step 1", 2));
    assert_eq!(split_reasoning_chunk("step 1\r\n\r\n"), ("step 1", 2));
}

#[test]
fn pure_newlines_return_empty_content_and_count() {
    assert_eq!(split_reasoning_chunk("\n"), ("", 1));
    assert_eq!(split_reasoning_chunk("\n\n\n"), ("", 3));
    assert_eq!(split_reasoning_chunk("  \n\n  \n"), ("", 3));
}

#[test]
fn internal_newlines_remain_in_content() {
    assert_eq!(split_reasoning_chunk("step 1\nstep 2"), ("step 1\nstep 2", 0));
    assert_eq!(split_reasoning_chunk("step 1\nstep 2\n\n"), ("step 1\nstep 2", 2));
}

#[test]
fn multibyte_characters_preserve_boundaries() {
    assert_eq!(split_reasoning_chunk("思考中\n\n"), ("思考中", 2));
    assert_eq!(split_reasoning_chunk("思考中 "), ("思考中 ", 0));
    assert_eq!(split_reasoning_chunk("🚀\n"), ("🚀", 1));
}

#[test]
fn empty_text_returns_empty_and_zero() {
    assert_eq!(split_reasoning_chunk(""), ("", 0));
}
