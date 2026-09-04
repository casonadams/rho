//! Line number resolution for edit diffs.

/// Attempts to locate the 1-based start line of an edit replacement in a file.
///
/// It checks `old_text` first (prior to the edit being applied on disk), and falls
/// back to `new_text` (if the edit has already been applied to the file on disk).
pub fn find_edit_line_number(path_str: &str, old_text: &str, new_text: &str) -> Option<usize> {
    let content = std::fs::read_to_string(path_str).ok()?;
    locate_match_line(&content, old_text).or_else(|| locate_match_line(&content, new_text))
}

fn locate_match_line(content: &str, target: &str) -> Option<usize> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(idx) = content.find(target) {
        return Some(1 + content[..idx].matches('\n').count());
    }
    let norm_content = content.replace("\r\n", "\n");
    let norm_target = target.replace("\r\n", "\n");
    if let Some(idx) = norm_content.find(&norm_target) {
        return Some(1 + norm_content[..idx].matches('\n').count());
    }
    let first_line = target.lines().find(|l| !l.trim().is_empty())?;
    if content.matches(first_line).count() == 1 {
        let idx = content.find(first_line)?;
        return Some(1 + content[..idx].matches('\n').count());
    }
    None
}
