use rho_harness_core::workspace::canonicalize_path;
use std::path::Path;

pub use rho_harness_core::workspace::expand_home;

pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let expanded_pattern = expand_home(pattern);
    let expanded_text = expand_home(text);
    if match_expanded(&expanded_pattern, &expanded_text) {
        return true;
    }
    canonical_prefix_pattern(&expanded_pattern).is_some_and(|anchored| match_expanded(&anchored, &expanded_text))
}

fn match_expanded(pattern: &str, text: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(" *") {
        return match_prefix_args(prefix, text);
    }
    if let Some(prefix) = pattern.strip_suffix("/*")
        && prefix.len() > 1
    {
        return match_dir_subpaths(prefix, text);
    }
    generic_wildcard(pattern, text)
}

/// Rewrites an absolute pattern's literal directory prefix to its symlink-free spelling,
/// so rules written against `/tmp` also match canonical `/private/tmp` paths.
fn canonical_prefix_pattern(pattern: &str) -> Option<String> {
    if !pattern.starts_with('/') {
        return None;
    }
    let literal_end = pattern.find(['*', '?']).unwrap_or(pattern.len());
    let (literal, rest) = pattern.split_at(literal_end);
    let dir_end = literal.rfind('/')? + 1;
    let (dir, name) = literal.split_at(dir_end);
    let mut anchored = canonicalize_path(Path::new(dir)).to_string_lossy().into_owned();
    if !anchored.ends_with('/') {
        anchored.push('/');
    }
    anchored.push_str(name);
    anchored.push_str(rest);
    (anchored != pattern).then_some(anchored)
}

fn match_prefix_args(prefix: &str, text: &str) -> bool {
    text == prefix || text.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
}

fn match_dir_subpaths(prefix: &str, text: &str) -> bool {
    text == prefix || text.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

fn generic_wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut state = (0usize, 0usize, None, 0usize);
    while state.1 < t.len() {
        if !advance_match(&p, &t, &mut state) {
            return false;
        }
    }
    p[state.0..].iter().all(|&c| c == '*')
}

type MatchState = (usize, usize, Option<usize>, usize);

fn advance_match(p: &[char], t: &[char], state: &mut MatchState) -> bool {
    let (pi, ti, star, star_at) = state;
    if *pi < p.len() && (p[*pi] == '?' || p[*pi] == t[*ti]) {
        *pi += 1;
        *ti += 1;
        return true;
    }
    if *pi < p.len() && p[*pi] == '*' {
        *star = Some(*pi);
        *star_at = *ti;
        *pi += 1;
        return true;
    }
    if let Some(sp) = *star {
        *star_at += 1;
        *pi = sp + 1;
        *ti = *star_at;
        return true;
    }
    false
}
