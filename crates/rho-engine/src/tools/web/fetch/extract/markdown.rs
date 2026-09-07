use std::sync::LazyLock;
use url::Url;

static FENCE_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)$").expect("valid regex"));

static REF_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(\s*\[[^\]]+\]:\s*)(\S+)(.*)$").expect("valid regex"));

static LINK_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(!?\[[^\]]*\]\()(<[^>]+>|(?:[^()\s]|\([^()]*\))+)([^)]*\))").expect("valid regex")
});

pub fn resolve_relative_url(target: &str, base_url: &str) -> String {
    if target.is_empty() || target.starts_with('#') {
        return target.to_string();
    }
    if target.starts_with("//") {
        let scheme = if base_url.starts_with("https:") {
            "https:"
        } else {
            "http:"
        };
        return format!("{scheme}{target}");
    }
    if target.contains("://") || target.starts_with("mailto:") || target.starts_with("tel:") {
        return target.to_string();
    }
    if let Ok(base) = Url::parse(base_url)
        && let Ok(resolved) = base.join(target)
    {
        return resolved.to_string();
    }
    target.to_string()
}

fn check_fence_toggle(line: &str, current_fence: &mut Option<(char, usize)>) -> bool {
    let Some(caps) = FENCE_PATTERN.captures(line) else {
        return false;
    };
    let marker_str = &caps[1];
    let marker_char = marker_str.chars().next().unwrap_or('`');
    let marker_len = marker_str.len();
    let tail = caps.get(2).map_or("", |m| m.as_str().trim());

    match *current_fence {
        None => {
            *current_fence = Some((marker_char, marker_len));
        }
        Some((c, len)) if c == marker_char && marker_len >= len && tail.is_empty() => {
            *current_fence = None;
        }
        _ => {}
    }
    true
}

fn rewrite_link_target(caps: &regex::Captures<'_>, base_url: &str) -> String {
    let prefix = &caps[1];
    let raw_target = &caps[2];
    let suffix = &caps[3];
    let (angled, target) = if raw_target.starts_with('<') && raw_target.ends_with('>') {
        (true, &raw_target[1..raw_target.len() - 1])
    } else {
        (false, raw_target)
    };
    let resolved = resolve_relative_url(target, base_url);
    if angled {
        format!("{prefix}<{resolved}>{suffix}")
    } else {
        format!("{prefix}{resolved}{suffix}")
    }
}

fn resolve_line_refs_and_links(line: &str, base_url: &str) -> String {
    let ref_replaced = if let Some(caps) = REF_PATTERN.captures(line) {
        let prefix = &caps[1];
        let target = &caps[2];
        let suffix = &caps[3];
        format!("{prefix}{}{suffix}", resolve_relative_url(target, base_url))
    } else {
        line.to_string()
    };

    LINK_PATTERN
        .replace_all(&ref_replaced, |caps: &regex::Captures| {
            rewrite_link_target(caps, base_url)
        })
        .into_owned()
}

pub fn resolve_markdown_links(markdown: &str, base_url: &str) -> String {
    let mut current_fence: Option<(char, usize)> = None;
    let mut out = Vec::new();

    for line in markdown.lines() {
        if check_fence_toggle(line, &mut current_fence) || current_fence.is_some() {
            out.push(line.to_string());
            continue;
        }
        out.push(resolve_line_refs_and_links(line, base_url));
    }

    out.join("\n")
}
