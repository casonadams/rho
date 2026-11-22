//! Turning a `line` + `symbol` the model names into an LSP position, and back.

use super::client::PositionEncoding;

/// The 0-based LSP character offset of byte `byte` within `line`.
pub fn character_offset(line: &str, byte: usize, encoding: PositionEncoding) -> u32 {
    let prefix = &line[..byte.min(line.len())];
    match encoding {
        PositionEncoding::Utf8 => prefix.len() as u32,
        PositionEncoding::Utf16 => prefix.encode_utf16().count() as u32,
    }
}

/// The byte index in `line` of LSP character offset `character`.
pub fn byte_index(line: &str, character: u32, encoding: PositionEncoding) -> usize {
    let mut units = 0u32;
    for (index, ch) in line.char_indices() {
        if units >= character {
            return index;
        }
        units += match encoding {
            PositionEncoding::Utf8 => ch.len_utf8() as u32,
            PositionEncoding::Utf16 => ch.len_utf16() as u32,
        };
    }
    line.len()
}

fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn whole_word_at(line: &str, start: usize, symbol: &str) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[start + symbol.len()..].chars().next();
    let bounded = |c: Option<char>| c.is_none_or(|c| !is_ident(c));
    !symbol.chars().all(is_ident) || (bounded(before) && bounded(after))
}

/// Byte index of `symbol` on `line`: the first whole-word occurrence, else the first substring
/// match, else a case-insensitive match. Without a symbol, the first non-blank character.
pub fn locate_symbol(line: &str, symbol: Option<&str>) -> Result<usize, String> {
    let Some(symbol) = symbol.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(line.len() - line.trim_start().len());
    };
    let matches: Vec<usize> = line.match_indices(symbol).map(|(index, _)| index).collect();
    if let Some(index) = matches.iter().copied().find(|&i| whole_word_at(line, i, symbol)) {
        return Ok(index);
    }
    if let Some(&index) = matches.first() {
        return Ok(index);
    }
    let wanted: Vec<char> = symbol.chars().flat_map(char::to_lowercase).collect();
    line.char_indices()
        .map(|(index, _)| index)
        .find(|&index| {
            let mut rest = line[index..].chars().flat_map(char::to_lowercase);
            wanted.iter().all(|want| rest.next() == Some(*want))
        })
        .ok_or_else(|| format!("'{symbol}' does not appear on that line: {}", line.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_resolve_to_whole_words_before_substrings() {
        let cases = [
            ("let foobar = foo(1);", Some("foo"), Ok(13)),
            ("let foobar = foo(1);", Some("bar"), Ok(7)),
            ("    self.run()", None, Ok(4)),
            ("Widget::new()", Some("widget"), Ok(0)),
            ("\u{130}x Foo", Some("foo"), Ok(4)),
            ("a.b", Some(".b"), Ok(1)),
        ];
        for (line, symbol, expected) in cases {
            assert_eq!(locate_symbol(line, symbol), expected, "{line} {symbol:?}");
        }
        assert!(
            locate_symbol("fn main()", Some("missing"))
                .unwrap_err()
                .contains("fn main()")
        );
    }

    #[test]
    fn offsets_follow_the_negotiated_encoding() {
        let line = "let s = \"\u{e9}\u{1F600}\"; s";
        let byte = line.rfind('s').unwrap();
        assert_eq!(character_offset(line, byte, PositionEncoding::Utf8), byte as u32);
        assert_eq!(character_offset(line, byte, PositionEncoding::Utf16), 15);
        for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
            let character = character_offset(line, byte, encoding);
            assert_eq!(byte_index(line, character, encoding), byte);
        }
    }
}
