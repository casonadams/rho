use std::path::PathBuf;

pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

pub fn expand_home(path: &str) -> String {
    let Some(home) = home_dir() else {
        return path.to_string();
    };
    let home_str = home.to_string_lossy();
    if path == "~" || path == "$HOME" {
        return home_str.to_string();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{home_str}/{rest}");
    }
    if let Some(rest) = path.strip_prefix("$HOME/") {
        return format!("{home_str}/{rest}");
    }
    path.to_string()
}

pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let expanded_pattern = expand_home(pattern);
    let expanded_text = expand_home(text);
    if let Some(prefix) = expanded_pattern.strip_suffix(" *") {
        return match_prefix_args(prefix, &expanded_text);
    }
    if let Some(prefix) = expanded_pattern.strip_suffix("/*")
        && prefix.len() > 1
    {
        return match_dir_subpaths(prefix, &expanded_text);
    }
    generic_wildcard(&expanded_pattern, &expanded_text)
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
