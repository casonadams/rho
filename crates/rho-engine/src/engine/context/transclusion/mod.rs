use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub const MAX_TRANSCLUSION_DEPTH: usize = 3;
pub const MAX_TRANSCLUSION_BYTES: usize = 64 * 1024;

struct TransclusionScope<'a> {
    depth: usize,
    visited: &'a mut HashSet<PathBuf>,
}

pub fn expand_transclusions(content: &str, base_dir: &Path) -> String {
    expand_transclusions_with_root(content, base_dir, None)
}

pub fn expand_transclusions_with_root(content: &str, base_dir: &Path, root_path: Option<&Path>) -> String {
    let mut visited = HashSet::new();
    if let Some(root) = root_path
        && let Ok(canonical) = root.canonicalize()
    {
        visited.insert(canonical);
    }
    let mut scope = TransclusionScope {
        depth: 0,
        visited: &mut visited,
    };
    expand_inner(content, base_dir, &mut scope)
}

fn expand_inner(content: &str, base_dir: &Path, scope: &mut TransclusionScope<'_>) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_code_fence = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if !in_code_fence && let Some(target) = parse_transclusion_target(trimmed) {
            let expanded = resolve_and_inline(target, base_dir, scope);
            result.push_str(&expanded);
            result.push('\n');
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn parse_transclusion_target(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('@')?;
    if rest.is_empty() {
        return None;
    }
    let target = rest.split([' ', '\t', '#']).next().unwrap_or("");
    if target.is_empty() { None } else { Some(target) }
}

fn resolve_and_inline(target_str: &str, base_dir: &Path, scope: &mut TransclusionScope<'_>) -> String {
    if scope.depth >= MAX_TRANSCLUSION_DEPTH {
        return format!("<!-- Transclusion depth limit exceeded: {target_str} -->");
    }

    let target_path = base_dir.join(target_str);
    let canonical = match target_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return format!("<!-- Transclusion failed: file not found: {target_str} -->");
        }
    };

    if !canonical.is_file() {
        return format!("<!-- Transclusion failed: file not found: {target_str} -->");
    }

    if !scope.visited.insert(canonical.clone()) {
        return format!("<!-- Transclusion loop detected: {target_str} -->");
    }

    let result = match read_bounded_file(&canonical, MAX_TRANSCLUSION_BYTES) {
        Ok((file_content, truncated)) => {
            let next_base = canonical.parent().unwrap_or(base_dir);
            let mut next_scope = TransclusionScope {
                depth: scope.depth + 1,
                visited: scope.visited,
            };
            let mut expanded = expand_inner(&file_content, next_base, &mut next_scope);
            if truncated {
                expanded.push_str(&format!("\n<!-- Transclusion truncated at 64 KB: {target_str} -->"));
            }
            expanded
        }
        Err(_) => {
            format!("<!-- Transclusion failed: file not readable: {target_str} -->")
        }
    };

    scope.visited.remove(&canonical);
    result
}

fn read_bounded_file(path: &Path, max_bytes: usize) -> std::io::Result<(String, bool)> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    let mut take = (&mut file).take(max_bytes as u64 + 1);
    take.read_to_end(&mut buf)?;
    let truncated = buf.len() > max_bytes;
    if truncated {
        buf.truncate(max_bytes);
    }
    let s = truncate_to_valid_utf8(&buf).to_string();
    Ok((s, truncated))
}

fn truncate_to_valid_utf8(mut bytes: &[u8]) -> &str {
    loop {
        match std::str::from_utf8(bytes) {
            Ok(s) => return s,
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to > 0 {
                    return std::str::from_utf8(&bytes[..valid_up_to]).unwrap_or("");
                }
                if bytes.is_empty() {
                    return "";
                }
                bytes = &bytes[..bytes.len() - 1];
            }
        }
    }
}
