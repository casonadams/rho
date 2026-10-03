// Hashline patch processor for line-anchored and content-hash validated edits.

pub fn compute_content_tag(content: &str) -> String {
    let mut hash: u32 = 0x811c9dc5;
    for byte in content.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{:04x}", (hash ^ (hash >> 16)) & 0xffff)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashlineOp {
    PutRange {
        start: usize,
        end: usize,
        lines: Vec<String>,
    },
    PutBefore {
        line: usize,
        lines: Vec<String>,
    },
    PutAfter {
        line: usize,
        lines: Vec<String>,
    },
    PutAppend {
        lines: Vec<String>,
    },
    CutRange {
        start: usize,
        end: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashlinePatch {
    pub tag: Option<String>,
    pub ops: Vec<HashlineOp>,
}

fn parse_line_num(s: &str) -> Option<usize> {
    s.trim().parse::<usize>().ok().filter(|&n| n > 0)
}

fn parse_header(line: &str) -> Option<Option<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let end_bracket = trimmed.find(']')?;
    let header_body = &trimmed[1..end_bracket];
    if let Some(hash_pos) = header_body.find('#') {
        Some(Some(header_body[hash_pos + 1..].trim().to_string()))
    } else {
        Some(Some(header_body.trim().to_string()))
    }
}

fn parse_cut_directive(rest: &str) -> Result<HashlineOp, String> {
    let parts: Vec<&str> = rest.split(".=").collect();
    if parts.len() == 2 {
        let start = parse_line_num(parts[0]).ok_or_else(|| format!("Invalid CUT start line: {}", parts[0]))?;
        let end = parse_line_num(parts[1]).ok_or_else(|| format!("Invalid CUT end line: {}", parts[1]))?;
        Ok(HashlineOp::CutRange { start, end })
    } else if parts.len() == 1 {
        let line_num = parse_line_num(parts[0]).ok_or_else(|| format!("Invalid CUT line: {}", parts[0]))?;
        Ok(HashlineOp::CutRange {
            start: line_num,
            end: line_num,
        })
    } else {
        Err(format!("Malformed CUT directive: {rest}"))
    }
}

fn parse_put_spec(spec: &str, new_lines: Vec<String>) -> Result<HashlineOp, String> {
    if spec == ">$" {
        return Ok(HashlineOp::PutAppend { lines: new_lines });
    }
    if let Some(after) = spec.strip_prefix('>') {
        let line = parse_line_num(after).ok_or_else(|| format!("Invalid PUT > line: {after}"))?;
        return Ok(HashlineOp::PutAfter { line, lines: new_lines });
    }
    if let Some(before) = spec.strip_prefix('<') {
        let line = parse_line_num(before).ok_or_else(|| format!("Invalid PUT < line: {before}"))?;
        return Ok(HashlineOp::PutBefore { line, lines: new_lines });
    }
    if spec.contains(".=") {
        let parts: Vec<&str> = spec.split(".=").collect();
        let start = parse_line_num(parts[0]).ok_or_else(|| format!("Invalid PUT start line: {}", parts[0]))?;
        let end = parse_line_num(parts[1]).ok_or_else(|| format!("Invalid PUT end line: {}", parts[1]))?;
        return Ok(HashlineOp::PutRange {
            start,
            end,
            lines: new_lines,
        });
    }
    let single = parse_line_num(spec).ok_or_else(|| format!("Malformed PUT directive: {spec}"))?;
    Ok(HashlineOp::PutRange {
        start: single,
        end: single,
        lines: new_lines,
    })
}

fn collect_put_lines<'a, I>(lines: &mut std::iter::Peekable<I>) -> Vec<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut new_lines = Vec::new();
    while let Some(peeked) = lines.peek() {
        if let Some(stripped) = peeked.strip_prefix('+') {
            new_lines.push(stripped.to_string());
            lines.next();
        } else if peeked.starts_with("PUT ") || peeked.starts_with("CUT ") || peeked.starts_with('[') {
            break;
        } else if peeked.trim().is_empty() {
            lines.next();
        } else {
            break;
        }
    }
    new_lines
}

pub fn parse_hashline_patch(patch_str: &str) -> Result<HashlinePatch, String> {
    let mut lines = patch_str.lines().peekable();
    let mut tag = None;

    if let Some(&first) = lines.peek()
        && let Some(parsed_tag) = parse_header(first)
    {
        tag = parsed_tag;
        lines.next();
    }

    let mut ops = Vec::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("CUT ") {
            ops.push(parse_cut_directive(rest)?);
        } else if let Some(rest) = trimmed.strip_prefix("PUT ") {
            let spec = rest.trim_end_matches(':').trim();
            let new_lines = collect_put_lines(&mut lines);
            ops.push(parse_put_spec(spec, new_lines)?);
        }
    }

    if ops.is_empty() {
        return Err("Patch contains no PUT or CUT operations".to_string());
    }

    Ok(HashlinePatch { tag, ops })
}

fn op_sort_key(op: &HashlineOp) -> usize {
    match op {
        HashlineOp::PutAppend { .. } => usize::MAX,
        HashlineOp::PutAfter { line, .. } | HashlineOp::PutBefore { line, .. } => *line,
        HashlineOp::PutRange { start, .. } | HashlineOp::CutRange { start, .. } => *start,
    }
}

fn apply_insert_op(
    line: usize,
    lines: Vec<String>,
    doc_lines: &mut Vec<String>,
    modified: &mut Vec<usize>,
    before: bool,
) -> Result<(), String> {
    let max_line = if before { doc_lines.len() + 1 } else { doc_lines.len() };
    if line < 1 || line > max_line {
        let op_name = if before { "PUT <" } else { "PUT >" };
        return Err(format!("{op_name} line {line} out of range (1..={max_line})"));
    }
    let idx = if before { line - 1 } else { line };
    let has_content = !lines.is_empty();
    for (offset, l) in lines.into_iter().enumerate() {
        doc_lines.insert(idx + offset, l);
    }
    if has_content {
        let mark_line = if before { line } else { line + 1 };
        modified.push(mark_line);
    }
    Ok(())
}

struct RangeOp<'a> {
    start: usize,
    end: usize,
    lines: Vec<String>,
    doc_lines: &'a mut Vec<String>,
    modified: &'a mut Vec<usize>,
    op_name: &'static str,
}

fn apply_range_op(p: RangeOp<'_>) -> Result<(), String> {
    if p.start < 1 || p.start > p.end || p.end > p.doc_lines.len() {
        return Err(format!(
            "{} {}.={} line range out of range (1..={})",
            p.op_name,
            p.start,
            p.end,
            p.doc_lines.len()
        ));
    }
    let idx = p.start - 1;
    let remove_count = p.end - p.start + 1;
    p.doc_lines.drain(idx..idx + remove_count);
    for (offset, l) in p.lines.into_iter().enumerate() {
        p.doc_lines.insert(idx + offset, l);
    }
    p.modified.push(p.start);
    Ok(())
}

fn apply_single_op(op: HashlineOp, doc_lines: &mut Vec<String>, modified: &mut Vec<usize>) -> Result<(), String> {
    match op {
        HashlineOp::PutAppend { lines } => {
            let start = doc_lines.len() + 1;
            doc_lines.extend(lines);
            modified.push(start);
            Ok(())
        }
        HashlineOp::PutBefore { line, lines } => apply_insert_op(line, lines, doc_lines, modified, true),
        HashlineOp::PutAfter { line, lines } => apply_insert_op(line, lines, doc_lines, modified, false),
        HashlineOp::PutRange { start, end, lines } => apply_range_op(RangeOp {
            start,
            end,
            lines,
            doc_lines,
            modified,
            op_name: "PUT",
        }),
        HashlineOp::CutRange { start, end } => apply_range_op(RangeOp {
            start,
            end,
            lines: Vec::new(),
            doc_lines,
            modified,
            op_name: "CUT",
        }),
    }
}

pub fn apply_hashline_patch(content: &str, patch: &HashlinePatch) -> Result<(String, Vec<usize>), String> {
    let expected_tag = compute_content_tag(content);
    if let Some(ref tag) = patch.tag
        && !tag.eq_ignore_ascii_case(&expected_tag)
    {
        return Err(format!(
            "File tag mismatch: patch specifies #{tag}, but current file tag is #{expected_tag}. Stale snapshot; re-read file before patching."
        ));
    }

    let mut doc_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut modified_lines = Vec::new();

    let mut sorted_ops = patch.ops.clone();
    sorted_ops.sort_by_key(|b| std::cmp::Reverse(op_sort_key(b)));

    for op in sorted_ops {
        apply_single_op(op, &mut doc_lines, &mut modified_lines)?;
    }

    modified_lines.sort_unstable();
    modified_lines.dedup();

    let mut result = doc_lines.join("\n");
    if content.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }

    Ok((result, modified_lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_content_tag() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let tag = compute_content_tag(content);
        assert_eq!(tag.len(), 4);
    }

    #[test]
    fn test_hashline_put_range() {
        let content = "line 1\nline 2\nline 3\nline 4\n";
        let tag = compute_content_tag(content);
        let patch_text = format!("[{tag}]\nPUT 2.=3:\n+new line 2\n+new line 3\n");
        let patch = parse_hashline_patch(&patch_text).unwrap();
        assert_eq!(patch.tag.as_deref(), Some(tag.as_str()));
        let (updated, lines) = apply_hashline_patch(content, &patch).unwrap();
        assert_eq!(updated, "line 1\nnew line 2\nnew line 3\nline 4\n");
        assert_eq!(lines, vec![2]);
    }

    #[test]
    fn test_hashline_cut_and_append() {
        let content = "line 1\nline 2\nline 3\n";
        let patch_text = "CUT 2\nPUT >$:\n+line 4\n";
        let patch = parse_hashline_patch(patch_text).unwrap();
        let (updated, _) = apply_hashline_patch(content, &patch).unwrap();
        assert_eq!(updated, "line 1\nline 3\nline 4\n");
    }

    #[test]
    fn test_hashline_tag_mismatch_fails() {
        let content = "hello world\n";
        let patch_text = "[foo#9999]\nPUT 1:\n+goodbye\n";
        let patch = parse_hashline_patch(patch_text).unwrap();
        let err = apply_hashline_patch(content, &patch).unwrap_err();
        assert!(err.contains("File tag mismatch"));
    }

    #[test]
    fn test_hashline_insert_ops() {
        let content = "line 1\nline 2\n";
        let patch_before = "PUT <2:\n+line 1.5\n";
        let patch_b = parse_hashline_patch(patch_before).unwrap();
        let (res_b, _) = apply_hashline_patch(content, &patch_b).unwrap();
        assert_eq!(res_b, "line 1\nline 1.5\nline 2\n");

        let patch_after = "PUT >1:\n+line 1.5\n";
        let patch_a = parse_hashline_patch(patch_after).unwrap();
        let (res_a, _) = apply_hashline_patch(content, &patch_a).unwrap();
        assert_eq!(res_a, "line 1\nline 1.5\nline 2\n");
    }
}
