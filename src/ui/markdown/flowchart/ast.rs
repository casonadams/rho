//! Flowchart AST representation and syntax parser.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TopToBottom,
    LeftToRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,
    Rounded,
    Diamond,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub lines: Vec<String>,
    pub shape: NodeShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Flowchart {
    pub fn parse(source: &str) -> Option<Self> {
        let mut direction = Direction::TopToBottom;
        let mut header_found = false;
        let mut nodes_map: HashMap<String, (Vec<String>, NodeShape)> = HashMap::new();
        let mut node_order: Vec<String> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();

        for raw_line in source.lines() {
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }

            for stmt in line.split(';') {
                let s = stmt.trim();
                if s.is_empty() {
                    continue;
                }

                if !header_found {
                    if let Some(dir) = parse_header(s) {
                        direction = dir;
                        header_found = true;
                        continue;
                    }
                    if s.starts_with("graph") || s.starts_with("flowchart") {
                        header_found = true;
                        continue;
                    }
                }

                if is_ignored_statement(s) {
                    continue;
                }

                parse_statement(s, &mut nodes_map, &mut node_order, &mut edges);
            }
        }

        if !header_found || nodes_map.is_empty() {
            return None;
        }

        let nodes = node_order
            .into_iter()
            .map(|id| {
                let (lines, shape) = nodes_map
                    .remove(&id)
                    .unwrap_or((vec![id.clone()], NodeShape::Rectangle));
                Node { id, lines, shape }
            })
            .collect();

        Some(Flowchart {
            direction,
            nodes,
            edges,
        })
    }
}

fn strip_comment(line: &str) -> &str {
    if let Some(idx) = line.find("%%") {
        &line[..idx]
    } else {
        line
    }
}

fn parse_header(s: &str) -> Option<Direction> {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("graph") || lower.starts_with("flowchart") {
        let mut parts = s.split_whitespace();
        parts.next(); // skip 'graph' or 'flowchart'
        if let Some(dir) = parts.next() {
            match dir.to_ascii_uppercase().as_str() {
                "LR" | "RL" => Some(Direction::LeftToRight),
                "TD" | "TB" | "BT" => Some(Direction::TopToBottom),
                _ => Some(Direction::TopToBottom),
            }
        } else {
            Some(Direction::TopToBottom)
        }
    } else {
        None
    }
}

fn is_ignored_statement(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("subgraph")
        || lower == "end"
        || lower.starts_with("classdef")
        || lower.starts_with("class ")
        || lower.starts_with("style ")
        || lower.starts_with("click ")
        || lower.starts_with("linkstyle ")
}

fn parse_statement(
    s: &str,
    nodes_map: &mut HashMap<String, (Vec<String>, NodeShape)>,
    node_order: &mut Vec<String>,
    edges: &mut Vec<Edge>,
) {
    // If the statement contains an arrow connector, split by arrow tokens
    let tokens = split_edge_chain(s);
    if tokens.len() >= 2 {
        for i in 0..tokens.len() - 1 {
            let from_raw = &tokens[i].0;
            let edge_label = tokens[i].1.clone();
            let to_raw = &tokens[i + 1].0;

            let from_id = register_node(from_raw, nodes_map, node_order);
            let to_id = register_node(to_raw, nodes_map, node_order);

            if !from_id.is_empty() && !to_id.is_empty() {
                edges.push(Edge {
                    from: from_id,
                    to: to_id,
                    label: edge_label,
                });
            }
        }
    } else {
        register_node(s, nodes_map, node_order);
    }
}

fn register_node(
    s: &str,
    nodes_map: &mut HashMap<String, (Vec<String>, NodeShape)>,
    node_order: &mut Vec<String>,
) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }

    let (id, lines_opt, shape) = parse_node_spec(s);
    if id.is_empty() {
        return String::new();
    }

    if let Some((existing_lines, existing_shape)) = nodes_map.get_mut(&id) {
        if let Some(new_lines) = lines_opt {
            *existing_lines = new_lines;
            *existing_shape = shape;
        }
    } else {
        let lines = lines_opt.unwrap_or_else(|| vec![id.clone()]);
        nodes_map.insert(id.clone(), (lines, shape));
        node_order.push(id.clone());
    }

    id
}

fn parse_node_spec(s: &str) -> (String, Option<Vec<String>>, NodeShape) {
    let s = s.trim();
    if let Some(open) = s.find('{')
        && let Some(close) = s.rfind('}')
        && close > open
    {
        let id = s[..open].trim().to_string();
        let lines = clean_label(&s[open + 1..close]);
        return (id, Some(lines), NodeShape::Diamond);
    }

    if let Some(open) = s.find('(')
        && let Some(close) = s.rfind(')')
        && close > open
    {
        let id = s[..open].trim().to_string();
        let inner = &s[open + 1..close];
        let lines = clean_label(inner.trim_start_matches('[').trim_end_matches(']'));
        return (id, Some(lines), NodeShape::Rounded);
    }

    if let Some(open) = s.find('[')
        && let Some(close) = s.rfind(']')
        && close > open
    {
        let id = s[..open].trim().to_string();
        let lines = clean_label(&s[open + 1..close]);
        return (id, Some(lines), NodeShape::Rectangle);
    }

    (s.to_string(), None, NodeShape::Rectangle)
}

fn clean_label(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let normalized = unquoted
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("<br />", "\n")
        .replace("\\n", "\n");

    let lines: Vec<String> = normalized
        .split('\n')
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        vec![s.trim().to_string()]
    } else {
        lines
    }
}

fn clean_single_label(s: &str) -> String {
    clean_label(s).join(" ")
}

fn split_edge_chain(s: &str) -> Vec<(String, Option<String>)> {
    let mut result: Vec<(String, Option<String>)> = Vec::new();
    let mut rest = s;

    while let Some((before, edge_label, after)) = find_next_arrow(rest) {
        result.push((before.trim().to_string(), edge_label));
        rest = after;
    }
    result.push((rest.trim().to_string(), None));
    result
}

fn find_next_arrow(s: &str) -> Option<(&str, Option<String>, &str)> {
    let arrow_patterns = ["==>", "-->", "-.->", "--o", "--x", "---"];

    let mut earliest: Option<(usize, usize)> = None;
    for pat in &arrow_patterns {
        if let Some(idx) = s.find(pat) {
            match earliest {
                Some((min_idx, _)) if idx < min_idx => {
                    earliest = Some((idx, pat.len()));
                }
                None => {
                    earliest = Some((idx, pat.len()));
                }
                _ => {}
            }
        }
    }

    let (arrow_start, arrow_len) = earliest?;
    let before = &s[..arrow_start];
    let mut after = &s[arrow_start + arrow_len..];

    // Check for inline label: -->|Label| or --> "Label"
    let mut label: Option<String> = None;
    let trimmed_after = after.trim_start();
    if let Some(rest_after) = trimmed_after.strip_prefix('|') {
        if let Some(close) = rest_after.find('|') {
            let lbl = clean_single_label(&rest_after[..close]);
            label = Some(lbl);
            let consumed = after.len() - trimmed_after.len() + 1 + close + 1;
            after = &after[consumed..];
        }
    } else if let Some(open) = before.rfind("--") {
        let candidate = before[open + 2..].trim();
        if !candidate.is_empty() && !candidate.starts_with('>') {
            label = Some(clean_single_label(candidate));
        }
    }

    Some((before, label, after))
}
