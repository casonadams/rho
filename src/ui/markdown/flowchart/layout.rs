//! Hierarchical flowchart layout and collision-free edge routing.

use super::ast::{Direction, Edge, Flowchart, Node, NodeShape};
use super::canvas::{ArrowDir, Canvas};
use std::collections::{HashMap, HashSet};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
struct LayoutNode {
    label: String,
    shape: NodeShape,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    rank: usize,
}

pub struct FlowchartLayout<'a> {
    flowchart: &'a Flowchart,
}

impl<'a> FlowchartLayout<'a> {
    pub fn new(flowchart: &'a Flowchart) -> Self {
        Self { flowchart }
    }

    pub fn render(&self) -> String {
        match self.flowchart.direction {
            Direction::TopToBottom => self.render_td(),
            Direction::LeftToRight => self.render_lr(),
        }
    }

    fn render_td(&self) -> String {
        let (forward_edges, back_edges) = partition_edges(&self.flowchart.nodes, &self.flowchart.edges);
        let ranks = compute_ranks(&self.flowchart.nodes, &forward_edges);
        let rank_groups = group_by_rank(&self.flowchart.nodes, &ranks);
        let node_sizes = compute_node_sizes(&self.flowchart.nodes);

        const H_GAP: usize = 4;
        let rank_widths = compute_td_rank_widths(&rank_groups, &node_sizes, H_GAP);
        let max_rank_width = rank_widths.iter().copied().max().unwrap_or(20);

        let left_gutter_needed = back_edges
            .iter()
            .any(|e| layout_nodes_rough_check(e, &rank_groups, &node_sizes, max_rank_width));
        let left_offset = if left_gutter_needed { 5 } else { 1 };

        let (layout_nodes, canvas_h) = layout_td_nodes(
            &rank_groups,
            &rank_widths,
            &node_sizes,
            &forward_edges,
            left_offset,
            max_rank_width,
        );

        let rightmost = layout_nodes
            .values()
            .map(|n| n.x + n.width)
            .max()
            .unwrap_or(max_rank_width);
        let gutter_base_x = rightmost + 4;
        let canvas_w = gutter_base_x + back_edges.len() * 4 + 4;

        let mut canvas = Canvas::new(canvas_w, canvas_h);
        draw_nodes(&mut canvas, layout_nodes.values());
        draw_td_forward_edges(&mut canvas, &layout_nodes, &forward_edges);
        draw_td_back_edges(
            &mut canvas,
            &layout_nodes,
            &back_edges,
            gutter_base_x,
            left_offset + max_rank_width / 2,
        );

        canvas.to_trimmed_string()
    }

    fn render_lr(&self) -> String {
        let (forward_edges, back_edges) = partition_edges(&self.flowchart.nodes, &self.flowchart.edges);
        let ranks = compute_ranks(&self.flowchart.nodes, &forward_edges);
        let rank_groups = group_by_rank(&self.flowchart.nodes, &ranks);
        let node_sizes = compute_node_sizes(&self.flowchart.nodes);

        let col_widths: Vec<usize> = rank_groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|n| node_sizes.get(&n.id).map(|s| s.0).unwrap_or(6))
                    .max()
                    .unwrap_or(6)
            })
            .collect();

        let (layout_nodes, current_x, max_y) = layout_lr_nodes(&rank_groups, &col_widths, &node_sizes, &forward_edges);

        let bottommost = layout_nodes.values().map(|n| n.y + n.height).max().unwrap_or(max_y);
        let gutter_base_y = bottommost + 2;
        let canvas_w = current_x + 2;
        let canvas_h = gutter_base_y + back_edges.len() * 3 + 3;

        let mut canvas = Canvas::new(canvas_w, canvas_h);
        draw_nodes(&mut canvas, layout_nodes.values());
        draw_lr_forward_edges(&mut canvas, &layout_nodes, &forward_edges);
        draw_lr_back_edges(&mut canvas, &layout_nodes, &back_edges, gutter_base_y);

        canvas.to_trimmed_string()
    }
}

fn compute_node_sizes(nodes: &[Node]) -> HashMap<String, (usize, usize)> {
    let mut map = HashMap::new();
    for node in nodes {
        let label_len = UnicodeWidthStr::width(node.label.as_str());
        let (w, h) = match node.shape {
            NodeShape::Diamond => ((label_len + 6).max(8), 3),
            NodeShape::Rectangle | NodeShape::Rounded => ((label_len + 4).max(6), 3),
        };
        map.insert(node.id.clone(), (w, h));
    }
    map
}

fn group_by_rank(nodes: &[Node], ranks: &HashMap<String, usize>) -> Vec<Vec<Node>> {
    let max_rank = ranks.values().copied().max().unwrap_or(0);
    let mut groups = vec![Vec::new(); max_rank + 1];
    for node in nodes {
        let r = *ranks.get(&node.id).unwrap_or(&0);
        groups[r].push(node.clone());
    }
    groups
}

fn compute_td_rank_widths(
    groups: &[Vec<Node>],
    node_sizes: &HashMap<String, (usize, usize)>,
    h_gap: usize,
) -> Vec<usize> {
    groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|n| node_sizes.get(&n.id).map(|s| s.0).unwrap_or(6))
                .sum::<usize>()
                + group.len().saturating_sub(1) * h_gap
        })
        .collect()
}

fn layout_td_nodes(
    groups: &[Vec<Node>],
    widths: &[usize],
    sizes: &HashMap<String, (usize, usize)>,
    forward_edges: &[Edge],
    left_offset: usize,
    max_w: usize,
) -> (HashMap<String, LayoutNode>, usize) {
    let mut nodes = HashMap::new();
    let mut current_y = 1;
    const H_GAP: usize = 4;

    for (r, group) in groups.iter().enumerate() {
        let rank_w = widths[r];
        let mut current_x = left_offset + (max_w.saturating_sub(rank_w)) / 2;

        for node in group {
            let (w, h) = *sizes.get(&node.id).unwrap_or(&(6, 3));
            nodes.insert(
                node.id.clone(),
                LayoutNode {
                    label: node.label.clone(),
                    shape: node.shape,
                    width: w,
                    height: h,
                    x: current_x,
                    y: current_y,
                    rank: r,
                },
            );
            current_x += w + H_GAP;
        }

        let has_label = forward_edges
            .iter()
            .any(|e| nodes.get(&e.from).is_some_and(|n| n.rank == r) && e.label.is_some());
        current_y += 3 + if has_label { 3 } else { 2 };
    }

    (nodes, current_y + 2)
}

fn layout_lr_nodes(
    groups: &[Vec<Node>],
    col_widths: &[usize],
    sizes: &HashMap<String, (usize, usize)>,
    forward_edges: &[Edge],
) -> (HashMap<String, LayoutNode>, usize, usize) {
    let mut nodes = HashMap::new();
    let mut current_x = 1;
    let mut max_y = 1;
    const V_GAP: usize = 2;

    for (r, group) in groups.iter().enumerate() {
        let col_w = col_widths[r];
        let mut current_y = 1;

        for node in group {
            let (w, h) = *sizes.get(&node.id).unwrap_or(&(6, 3));
            let node_x = current_x + (col_w.saturating_sub(w)) / 2;
            nodes.insert(
                node.id.clone(),
                LayoutNode {
                    label: node.label.clone(),
                    shape: node.shape,
                    width: w,
                    height: h,
                    x: node_x,
                    y: current_y,
                    rank: r,
                },
            );
            current_y += h + V_GAP;
        }

        max_y = max_y.max(current_y);
        let has_label = forward_edges
            .iter()
            .any(|e| nodes.get(&e.from).is_some_and(|n| n.rank == r) && e.label.is_some());
        current_x += col_w + if has_label { 7 } else { 6 };
    }

    (nodes, current_x, max_y)
}

fn draw_nodes<'a>(canvas: &mut Canvas, nodes: impl Iterator<Item = &'a LayoutNode>) {
    for node in nodes {
        match node.shape {
            NodeShape::Rectangle => {
                canvas.draw_rect_box(node.x, node.y, node.width, node.height, &node.label);
            }
            NodeShape::Rounded => {
                canvas.draw_rounded_box(node.x, node.y, node.width, node.height, &node.label);
            }
            NodeShape::Diamond => {
                canvas.draw_diamond_box(node.x, node.y, node.width, node.height, &node.label);
            }
        }
    }
}

fn draw_td_forward_edges(canvas: &mut Canvas, nodes: &HashMap<String, LayoutNode>, edges: &[Edge]) {
    for edge in edges {
        let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) else {
            continue;
        };
        let from_cx = from.x + from.width / 2;
        let from_by = from.y + from.height - 1;
        let to_cx = to.x + to.width / 2;
        let to_ty = to.y;

        if to_ty <= from_by {
            continue;
        }

        if from_cx == to_cx || (from_cx as isize - to_cx as isize).abs() <= 1 {
            let cx = to_cx;
            canvas.draw_v_line(cx, from_by + 1, to_ty.saturating_sub(2));
            canvas.draw_arrow(cx, to_ty.saturating_sub(1), ArrowDir::Down);
            if let Some(label) = &edge.label {
                let label_y = from_by + (to_ty.saturating_sub(from_by)) / 2;
                canvas.draw_text(cx + 2, label_y, label);
            }
        } else {
            let mid_y = from_by + 1;
            canvas.draw_v_line(from_cx, from_by + 1, mid_y);
            canvas.draw_h_line(mid_y, from_cx, to_cx);
            canvas.draw_v_line(to_cx, mid_y, to_ty.saturating_sub(2));
            canvas.draw_arrow(to_cx, to_ty.saturating_sub(1), ArrowDir::Down);
            if let Some(label) = &edge.label {
                let min_x = from_cx.min(to_cx);
                canvas.draw_text(min_x + 2, mid_y.saturating_sub(1), label);
            }
        }
    }
}

fn draw_td_back_edges(
    canvas: &mut Canvas,
    nodes: &HashMap<String, LayoutNode>,
    edges: &[Edge],
    gutter_base_x: usize,
    center_split: usize,
) {
    let mut left_gutter_idx = 0;
    let mut right_gutter_idx = 0;

    for edge in edges {
        let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) else {
            continue;
        };
        let from_cx = from.x + from.width / 2;
        let from_mid_y = from.y + from.height / 2;
        let to_mid_y = to.y + to.height / 2;

        if from_cx < center_split {
            let gutter_x = 2 + left_gutter_idx * 3;
            left_gutter_idx += 1;
            canvas.draw_h_line(from_mid_y, gutter_x, from.x.saturating_sub(1));
            canvas.draw_v_line(gutter_x, from_mid_y, to_mid_y);
            canvas.draw_h_line(to_mid_y, gutter_x, to.x.saturating_sub(2));
            canvas.draw_arrow(to.x.saturating_sub(1), to_mid_y, ArrowDir::Right);
            if let Some(label) = &edge.label {
                canvas.draw_text(gutter_x + 1, from_mid_y.saturating_sub(1), label);
            }
        } else {
            let gutter_x = gutter_base_x + right_gutter_idx * 4;
            right_gutter_idx += 1;
            let from_rx = from.x + from.width - 1;
            let to_rx = to.x + to.width - 1;
            canvas.draw_h_line(from_mid_y, from_rx + 1, gutter_x);
            canvas.draw_v_line(gutter_x, from_mid_y, to_mid_y);
            canvas.draw_h_line(to_mid_y, to_rx + 2, gutter_x);
            canvas.draw_arrow(to_rx + 1, to_mid_y, ArrowDir::Left);
            if let Some(label) = &edge.label {
                canvas.draw_text(from_rx + 2, from_mid_y.saturating_sub(1), label);
            }
        }
    }
}

fn draw_lr_forward_edges(canvas: &mut Canvas, nodes: &HashMap<String, LayoutNode>, edges: &[Edge]) {
    for edge in edges {
        let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) else {
            continue;
        };
        let from_rx = from.x + from.width - 1;
        let from_my = from.y + from.height / 2;
        let to_lx = to.x;
        let to_my = to.y + to.height / 2;

        if to_lx <= from_rx {
            continue;
        }

        if from_my == to_my || (from_my as isize - to_my as isize).abs() <= 1 {
            let my = to_my;
            canvas.draw_h_line(my, from_rx + 1, to_lx.saturating_sub(2));
            canvas.draw_arrow(to_lx.saturating_sub(1), my, ArrowDir::Right);
            if let Some(label) = &edge.label {
                let label_x = from_rx + (to_lx.saturating_sub(from_rx)) / 2;
                canvas.draw_text(label_x, from_my.saturating_sub(1), label);
            }
        } else {
            let mid_x = from_rx + 2;
            canvas.draw_h_line(from_my, from_rx + 1, mid_x);
            canvas.draw_v_line(mid_x, from_my, to_my);
            canvas.draw_h_line(to_my, mid_x, to_lx.saturating_sub(2));
            canvas.draw_arrow(to_lx.saturating_sub(1), to_my, ArrowDir::Right);
            if let Some(label) = &edge.label {
                canvas.draw_text(mid_x + 1, to_my.saturating_sub(1), label);
            }
        }
    }
}

fn draw_lr_back_edges(canvas: &mut Canvas, nodes: &HashMap<String, LayoutNode>, edges: &[Edge], gutter_base_y: usize) {
    for (i, edge) in edges.iter().enumerate() {
        let (Some(from), Some(to)) = (nodes.get(&edge.from), nodes.get(&edge.to)) else {
            continue;
        };
        let gutter_y = gutter_base_y + i * 3;
        let from_cx = from.x + from.width / 2;
        let from_by = from.y + from.height - 1;
        let to_cx = to.x + to.width / 2;
        let to_by = to.y + to.height - 1;

        canvas.draw_v_line(from_cx, from_by + 1, gutter_y);
        canvas.draw_h_line(gutter_y, from_cx, to_cx);
        canvas.draw_v_line(to_cx, gutter_y, to_by + 2);
        canvas.draw_arrow(to_cx, to_by + 1, ArrowDir::Up);

        if let Some(label) = &edge.label {
            let min_x = from_cx.min(to_cx);
            canvas.draw_text(min_x + 2, gutter_y.saturating_sub(1), label);
        }
    }
}

fn partition_edges(nodes: &[Node], edges: &[Edge]) -> (Vec<Edge>, Vec<Edge>) {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in edges {
        adj.entry(&edge.from).or_default().push(&edge.to);
    }

    let mut visited: HashMap<&str, u8> = HashMap::new();
    let mut back_edge_set: HashSet<(&str, &str)> = HashSet::new();

    fn dfs<'a>(
        u: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashMap<&'a str, u8>,
        back_edge_set: &mut HashSet<(&'a str, &'a str)>,
    ) {
        visited.insert(u, 1);
        if let Some(neighbors) = adj.get(u) {
            for &v in neighbors {
                match visited.get(v).copied().unwrap_or(0) {
                    1 => {
                        back_edge_set.insert((u, v));
                    }
                    0 => {
                        dfs(v, adj, visited, back_edge_set);
                    }
                    _ => {}
                }
            }
        }
        visited.insert(u, 2);
    }

    for node in nodes {
        if visited.get(node.id.as_str()).copied().unwrap_or(0) == 0 {
            dfs(&node.id, &adj, &mut visited, &mut back_edge_set);
        }
    }

    let mut forward = Vec::new();
    let mut back = Vec::new();
    for edge in edges {
        if back_edge_set.contains(&(edge.from.as_str(), edge.to.as_str())) {
            back.push(edge.clone());
        } else {
            forward.push(edge.clone());
        }
    }

    (forward, back)
}

fn compute_ranks(nodes: &[Node], forward_edges: &[Edge]) -> HashMap<String, usize> {
    let mut in_degrees: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in nodes {
        in_degrees.insert(&node.id, 0);
    }
    for edge in forward_edges {
        *in_degrees.entry(&edge.to).or_insert(0) += 1;
        adj.entry(&edge.from).or_default().push(&edge.to);
    }

    let mut ranks: HashMap<String, usize> = HashMap::new();
    for node in nodes {
        ranks.insert(node.id.clone(), 0);
    }

    let mut queue: Vec<&str> = in_degrees
        .iter()
        .filter_map(|(&id, &deg)| if deg == 0 { Some(id) } else { None })
        .collect();

    while let Some(u) = queue.pop() {
        let u_rank = *ranks.get(u).unwrap_or(&0);
        if let Some(neighbors) = adj.get(u) {
            for &v in neighbors {
                let v_rank = ranks.get(v).copied().unwrap_or(0);
                if u_rank + 1 > v_rank {
                    ranks.insert(v.to_string(), u_rank + 1);
                }
                if let Some(deg) = in_degrees.get_mut(v) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push(v);
                    }
                }
            }
        }
    }

    ranks
}

fn layout_nodes_rough_check(
    edge: &Edge,
    rank_groups: &[Vec<Node>],
    node_sizes: &HashMap<String, (usize, usize)>,
    max_rank_width: usize,
) -> bool {
    for group in rank_groups {
        let mut x = 0;
        for node in group {
            let w = node_sizes.get(&node.id).map(|s| s.0).unwrap_or(6);
            if node.id == edge.from {
                return (x + w / 2) < max_rank_width / 2;
            }
            x += w + 4;
        }
    }
    false
}
