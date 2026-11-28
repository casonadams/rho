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

pub fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(key, _)| {
        key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

pub(super) fn is_duration(word: &str) -> bool {
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
    use test_case::test_case;

    fn normalized(command: &str) -> Option<String> {
        normalize_segment(&tokenize(command).tokens)
    }

    #[test_case("rm -rf build", "rm -rf build"; "plain")]
    #[test_case("\"rm\" -rf build", "rm -rf build"; "double quoted")]
    #[test_case("r''m -rf build", "rm -rf build"; "empty quotes inside")]
    #[test_case("\\rm -rf build", "rm -rf build"; "backslash escaped")]
    #[test_case("/bin/rm -rf build", "rm -rf build"; "absolute path")]
    #[test_case("env rm -rf build", "rm -rf build"; "env")]
    #[test_case("env -i FOO=1 rm -rf build", "rm -rf build"; "env with assignment")]
    #[test_case("env -u HOME rm -rf build", "rm -rf build"; "env unset")]
    #[test_case("sudo -u root rm -rf build", "rm -rf build"; "sudo user")]
    #[test_case("sudo -- rm -rf build", "rm -rf build"; "sudo double dash")]
    #[test_case("xargs -0 rm -rf", "rm -rf"; "xargs")]
    #[test_case("timeout 5 rm -rf build", "rm -rf build"; "timeout")]
    #[test_case("exec nice -n 5 rm -rf build", "rm -rf build"; "exec nice")]
    #[test_case("{ rm -rf build", "rm -rf build"; "brace group")]
    #[test_case("then rm -rf build", "rm -rf build"; "then keyword")]
    #[test_case("! rm -rf build", "rm -rf build"; "negation")]
    #[test_case("rm -rf build > /dev/null", "rm -rf build"; "redirect")]
    fn quoting_paths_and_wrappers_reduce_to_the_command(input: &str, expected: &str) {
        assert_eq!(normalized(input).as_deref(), Some(expected));
    }

    #[test_case("timeout -s KILL 5 rm -rf build", "rm -rf build"; "timeout signal")]
    #[test_case("timeout -k 2 5 rm -rf build", "rm -rf build"; "timeout kill after")]
    #[test_case("xargs -n 1 rm -rf", "rm -rf"; "xargs max args")]
    #[test_case("xargs -I X rm -rf", "rm -rf"; "xargs replace")]
    #[test_case("stdbuf -o L rm -rf build", "rm -rf build"; "stdbuf")]
    #[test_case("nice -n 5 rm -rf build", "rm -rf build"; "nice")]
    fn wrapper_options_that_take_values_are_skipped(input: &str, expected: &str) {
        assert_eq!(normalized(input).as_deref(), Some(expected));
    }

    #[test_case("env 5 rm -rf build", "5 rm -rf build"; "env keeps number")]
    #[test_case("timeout abc rm -rf build", "abc rm -rf build"; "timeout needs duration")]
    #[test_case("=x rm -rf build", "=x rm -rf build"; "empty name")]
    #[test_case("1A=x rm -rf build", "1A=x rm -rf build"; "leading digit name")]
    #[test_case("A-B=x rm -rf build", "A-B=x rm -rf build"; "dash in name")]
    #[test_case("_A1=x rm -rf build", "rm -rf build"; "valid name is stripped")]
    fn only_timeout_consumes_a_duration_and_only_valid_names_are_assignments(input: &str, expected: &str) {
        assert_eq!(normalized(input).as_deref(), Some(expected));
    }

    #[test]
    fn segments_with_only_prefixes_normalize_to_nothing() {
        assert_eq!(normalized("}"), Some("}".to_string()));
        assert_eq!(normalized("then"), None);
    }
}
