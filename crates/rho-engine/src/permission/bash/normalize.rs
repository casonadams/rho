//! Canonical form of a command segment for deny-rule matching: quotes and escapes
//! resolved, the command word reduced to its basename, and shell keywords and
//! privilege/exec wrappers removed, so `"/bin/rm" -rf x` matches a `rm -rf *` rule.

use super::lexer::{Token, TokenKind};

const KEYWORDS: &[&str] = &["{", "(", "!", "if", "then", "do", "else", "elif", "while", "until"];

const WRAPPERS: &[&str] = &[
    "env", "sudo", "doas", "exec", "command", "builtin", "nohup", "nice", "noglob", "time", "timeout", "xargs",
    "stdbuf", "setsid",
];

fn option_takes_value(wrapper: &str, option: &str) -> bool {
    let valued: &[&str] = match wrapper {
        "env" => &["-u", "-C", "-S"],
        "sudo" | "doas" => &["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U"],
        "nice" => &["-n"],
        "timeout" => &["-s", "-k"],
        "xargs" => &["-I", "-n", "-L", "-P", "-s", "-d", "-E", "-a"],
        "stdbuf" => &["-i", "-o", "-e"],
        _ => &[],
    };
    valued.contains(&option)
}

fn basename(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(key, _)| {
        key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn is_duration(word: &str) -> bool {
    let trimmed = word.trim_end_matches(['s', 'm', 'h', 'd']);
    !trimmed.is_empty() && trimmed.parse::<f64>().is_ok()
}

fn words_without_redirects(tokens: &[Token]) -> Vec<&str> {
    let mut words = Vec::new();
    let mut skip_target = false;
    for token in tokens {
        match token.kind {
            TokenKind::Redirect => skip_target = true,
            TokenKind::Word if skip_target => skip_target = false,
            TokenKind::Word => words.push(token.text.as_str()),
            TokenKind::Separator => {}
        }
    }
    words
}

fn skip_wrapper_arguments(words: &[&str], mut index: usize, wrapper: &str) -> usize {
    while let Some(word) = words.get(index).copied() {
        if word == "--" {
            return index + 1;
        }
        if !word.starts_with('-') {
            break;
        }
        index += if option_takes_value(wrapper, word) { 2 } else { 1 };
    }
    if wrapper == "timeout" && words.get(index).is_some_and(|w| is_duration(w)) {
        index += 1;
    }
    index
}

fn skip_prefixes(words: &[&str]) -> usize {
    let mut index = 0;
    while let Some(word) = words.get(index).copied() {
        let name = basename(word);
        if KEYWORDS.contains(&word) || is_assignment(word) {
            index += 1;
        } else if WRAPPERS.contains(&name) {
            index = skip_wrapper_arguments(words, index + 1, name);
        } else {
            break;
        }
    }
    index
}

/// Returns `None` when nothing remains after stripping prefixes (for example a lone `}`).
pub(crate) fn normalize_segment(tokens: &[Token]) -> Option<String> {
    let words = words_without_redirects(tokens);
    let start = skip_prefixes(&words);
    let (command, args) = words.get(start..)?.split_first()?;
    let mut parts = vec![basename(command)];
    parts.extend(args);
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::bash::lexer::tokenize;

    fn normalized(command: &str) -> Option<String> {
        normalize_segment(&tokenize(command).tokens)
    }

    #[test]
    fn quoting_paths_and_wrappers_reduce_to_the_command() {
        let cases = [
            ("rm -rf build", "rm -rf build"),
            ("\"rm\" -rf build", "rm -rf build"),
            ("r''m -rf build", "rm -rf build"),
            ("\\rm -rf build", "rm -rf build"),
            ("/bin/rm -rf build", "rm -rf build"),
            ("env rm -rf build", "rm -rf build"),
            ("env -i FOO=1 rm -rf build", "rm -rf build"),
            ("env -u HOME rm -rf build", "rm -rf build"),
            ("sudo -u root rm -rf build", "rm -rf build"),
            ("sudo -- rm -rf build", "rm -rf build"),
            ("xargs -0 rm -rf", "rm -rf"),
            ("timeout 5 rm -rf build", "rm -rf build"),
            ("exec nice -n 5 rm -rf build", "rm -rf build"),
            ("{ rm -rf build", "rm -rf build"),
            ("then rm -rf build", "rm -rf build"),
            ("! rm -rf build", "rm -rf build"),
            ("rm -rf build > /dev/null", "rm -rf build"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalized(input).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn wrapper_options_that_take_values_are_skipped() {
        let cases = [
            ("timeout -s KILL 5 rm -rf build", "rm -rf build"),
            ("timeout -k 2 5 rm -rf build", "rm -rf build"),
            ("xargs -n 1 rm -rf", "rm -rf"),
            ("xargs -I X rm -rf", "rm -rf"),
            ("stdbuf -o L rm -rf build", "rm -rf build"),
            ("nice -n 5 rm -rf build", "rm -rf build"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalized(input).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn only_timeout_consumes_a_duration_and_only_valid_names_are_assignments() {
        let kept = [
            ("env 5 rm -rf build", "5 rm -rf build"),
            ("timeout abc rm -rf build", "abc rm -rf build"),
            ("=x rm -rf build", "=x rm -rf build"),
            ("1A=x rm -rf build", "1A=x rm -rf build"),
            ("A-B=x rm -rf build", "A-B=x rm -rf build"),
        ];
        for (input, expected) in kept {
            assert_eq!(normalized(input).as_deref(), Some(expected), "{input}");
        }
        assert_eq!(normalized("_A1=x rm -rf build").as_deref(), Some("rm -rf build"));
    }

    #[test]
    fn segments_with_only_prefixes_normalize_to_nothing() {
        assert_eq!(normalized("}"), Some("}".to_string()));
        assert_eq!(normalized("then"), None);
    }
}
