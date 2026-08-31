use crate::ui::interactive::SPINNER_FRAMES;
use crate::ui::interactive::footer::truncate_to_width;
use crate::ui::theme::Theme;

pub const DEFAULT_MAX_AGENT_LINES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Queued,
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentDisplayItem {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub turn_count: usize,
    pub max_turns: Option<usize>,
    pub tool_uses: usize,
    pub tokens: u64,
    pub duration_ms: u64,
    pub activity: Option<String>,
    pub linger_turns: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct SubagentOverlayOptions<'a> {
    pub theme: &'a Theme,
    pub spinner_frame: usize,
    pub width: usize,
    pub max_lines: usize,
    pub expanded: bool,
}

impl<'a> SubagentOverlayOptions<'a> {
    pub fn new(theme: &'a Theme, spinner_frame: usize, width: usize) -> Self {
        Self {
            theme,
            spinner_frame,
            width,
            max_lines: DEFAULT_MAX_AGENT_LINES,
            expanded: false,
        }
    }

    pub fn with_limits(mut self, max_lines: usize, expanded: bool) -> Self {
        self.max_lines = max_lines;
        self.expanded = expanded;
        self
    }
}

pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M token", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k token", count as f64 / 1_000.0)
    } else {
        format!("{count} token")
    }
}

pub fn format_duration_ms(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

pub fn format_turns(turn_count: usize, max_turns: Option<usize>) -> String {
    match max_turns {
        Some(max) => format!("↻{turn_count}≤{max}"),
        None => format!("↻{turn_count}"),
    }
}

pub fn format_finished_line(item: &SubagentDisplayItem, prefix: &str, opts: &SubagentOverlayOptions<'_>) -> String {
    let theme = opts.theme;
    let dim = theme.dimmed;
    let icon = if item.status == SubagentStatus::Completed {
        let ok = theme.tool_ok;
        format!("{ok}✓{ok:#}")
    } else {
        let err = theme.tool_err;
        format!("{err}✗{err:#}")
    };

    let mut parts = Vec::new();
    if item.turn_count > 0 {
        parts.push(format_turns(item.turn_count, item.max_turns));
    }
    if item.tool_uses > 0 {
        parts.push(format!(
            "{} tool{}",
            item.tool_uses,
            if item.tool_uses == 1 { "" } else { "s" }
        ));
    }
    if item.tokens > 0 {
        parts.push(format_tokens(item.tokens));
    }
    parts.push(format_duration_ms(item.duration_ms));

    let stats = parts.join(" · ");
    let line = format!(
        "{dim}{prefix}{dim:#} {icon} {} {dim}{}{dim:#} {dim}·{dim:#} {dim}{stats}{dim:#}",
        item.agent_type, item.description
    );
    truncate_to_width(&line, opts.width)
}

pub fn format_running_lines(
    item: &SubagentDisplayItem,
    is_last: bool,
    opts: &SubagentOverlayOptions<'_>,
) -> [String; 2] {
    let theme = opts.theme;
    let dim = theme.dimmed;
    let accent = theme.highlight;
    let spinner_char = SPINNER_FRAMES[opts.spinner_frame % SPINNER_FRAMES.len()];
    let spinner = format!("{accent}{spinner_char}{accent:#}");

    let mut parts = Vec::new();
    if item.turn_count > 0 {
        parts.push(format_turns(item.turn_count, item.max_turns));
    }
    if item.tool_uses > 0 {
        parts.push(format!(
            "{} tool{}",
            item.tool_uses,
            if item.tool_uses == 1 { "" } else { "s" }
        ));
    }
    if item.tokens > 0 {
        parts.push(format_tokens(item.tokens));
    }
    parts.push(format_duration_ms(item.duration_ms));

    let stats = parts.join(" · ");
    let header_prefix = if is_last { "└─" } else { "├─" };
    let agent_name = format!("{accent}{}{accent:#}", item.agent_type);
    let header = format!(
        "{dim}{header_prefix}{dim:#} {spinner} {agent_name}  {dim}{}{dim:#} {dim}·{dim:#} {dim}{stats}{dim:#}",
        item.description
    );

    let activity = item.activity.as_deref().unwrap_or("thinking...");
    let sub_prefix = if is_last { "   " } else { "│  " };
    let sub_line = format!("{dim}{sub_prefix}  ⎿  {activity}{dim:#}");

    [
        truncate_to_width(&header, opts.width),
        truncate_to_width(&sub_line, opts.width),
    ]
}

pub fn format_subagent_overlay(agents: &[SubagentDisplayItem], opts: &SubagentOverlayOptions<'_>) -> Vec<String> {
    if agents.is_empty() {
        return Vec::new();
    }

    let running: Vec<&SubagentDisplayItem> = agents.iter().filter(|a| a.status == SubagentStatus::Running).collect();
    let queued_count = agents.iter().filter(|a| a.status == SubagentStatus::Queued).count();
    let finished: Vec<&SubagentDisplayItem> = agents
        .iter()
        .filter(|a| a.status == SubagentStatus::Completed || a.status == SubagentStatus::Error)
        .collect();

    let has_active = !running.is_empty() || queued_count > 0;
    let (heading_icon, heading_color) = if has_active {
        ("●", opts.theme.highlight)
    } else {
        ("○", opts.theme.dimmed)
    };

    let heading = truncate_to_width(
        &format!("{heading_color}{heading_icon} Agents{heading_color:#}"),
        opts.width,
    );
    let mut lines = vec![heading];

    let max_body = if opts.expanded {
        usize::MAX
    } else {
        opts.max_lines.saturating_sub(1).max(1)
    };

    let total_body_needed = finished.len() + running.len() * 2 + usize::from(queued_count > 0);

    if total_body_needed <= max_body {
        let total_items = finished.len() + running.len() + usize::from(queued_count > 0);
        let mut item_index = 0;

        for item in &finished {
            item_index += 1;
            let is_last = item_index == total_items;
            let prefix = if is_last { "└─" } else { "├─" };
            lines.push(format_finished_line(item, prefix, opts));
        }

        for item in &running {
            item_index += 1;
            let is_last = item_index == total_items;
            let [h, s] = format_running_lines(item, is_last, opts);
            lines.push(h);
            lines.push(s);
        }

        if queued_count > 0 {
            let dim = opts.theme.dimmed;
            let queued_line = format!("{dim}└─ ◦ {queued_count} queued{dim:#}");
            lines.push(truncate_to_width(&queued_line, opts.width));
        }
    } else {
        let mut budget = max_body.saturating_sub(1).max(1);
        let mut hidden_running = 0;
        let mut hidden_finished = 0;

        let queued_reserve = if queued_count > 0 { 1 } else { 0 };
        budget = budget.saturating_sub(queued_reserve);

        let mut shown_running = Vec::new();
        for item in &running {
            if budget >= 2 {
                shown_running.push(*item);
                budget -= 2;
            } else {
                hidden_running += 1;
            }
        }

        budget += queued_reserve;
        let show_queued = queued_count > 0 && budget >= 1;
        if show_queued {
            budget -= 1;
        }

        let mut shown_finished = Vec::new();
        for item in &finished {
            if budget >= 1 {
                shown_finished.push(*item);
                budget -= 1;
            } else {
                hidden_finished += 1;
            }
        }

        for item in &shown_finished {
            lines.push(format_finished_line(item, "├─", opts));
        }

        for item in &shown_running {
            let [h, s] = format_running_lines(item, false, opts);
            lines.push(h);
            lines.push(s);
        }

        if show_queued {
            let dim = opts.theme.dimmed;
            let queued_line = format!("{dim}├─ ◦ {queued_count} queued{dim:#}");
            lines.push(truncate_to_width(&queued_line, opts.width));
        }

        let dim = opts.theme.dimmed;
        let total_hidden = hidden_running + hidden_finished;
        let mut parts = Vec::new();
        if hidden_running > 0 {
            parts.push(format!("{hidden_running} running"));
        }
        if hidden_finished > 0 {
            parts.push(format!("{hidden_finished} finished"));
        }
        let summary = if parts.is_empty() {
            format!("+{total_hidden} more")
        } else {
            format!("+{total_hidden} more ({})", parts.join(", "))
        };
        lines.push(truncate_to_width(&format!("{dim}└─ {summary}{dim:#}"), opts.width));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_agents_returns_empty() {
        let theme = Theme::default();
        let opts = SubagentOverlayOptions::new(&theme, 0, 80);
        let lines = format_subagent_overlay(&[], &opts);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_running_and_finished_subagents() {
        let theme = Theme::default();
        let opts = SubagentOverlayOptions::new(&theme, 0, 80);
        let agents = vec![
            SubagentDisplayItem {
                id: "job_1".to_string(),
                agent_type: "explore".to_string(),
                description: "Fast code search".to_string(),
                status: SubagentStatus::Completed,
                turn_count: 2,
                max_turns: Some(10),
                tool_uses: 4,
                tokens: 12_300,
                duration_ms: 4_200,
                activity: None,
                linger_turns: 0,
            },
            SubagentDisplayItem {
                id: "job_2".to_string(),
                agent_type: "plan".to_string(),
                description: "Design implementation".to_string(),
                status: SubagentStatus::Running,
                turn_count: 1,
                max_turns: None,
                tool_uses: 1,
                tokens: 4_100,
                duration_ms: 1_800,
                activity: Some("reading crates/rho-core/src/provider.rs".to_string()),
                linger_turns: 0,
            },
        ];

        let lines = format_subagent_overlay(&agents, &opts);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("● Agents"));
        assert!(lines[1].contains("├─"));
        assert!(lines[1].contains("✓"));
        assert!(lines[1].contains("explore"));
        assert!(lines[1].contains("12.3k token"));
        assert!(lines[2].contains("└─"));
        assert!(lines[2].contains("plan"));
        assert!(lines[3].contains("⎿  reading crates/rho-core/src/provider.rs"));
    }

    #[test]
    fn test_queued_subagents_line() {
        let theme = Theme::default();
        let opts = SubagentOverlayOptions::new(&theme, 0, 80);
        let agents = vec![SubagentDisplayItem {
            id: "job_1".to_string(),
            agent_type: "explore".to_string(),
            description: "Search".to_string(),
            status: SubagentStatus::Queued,
            turn_count: 0,
            max_turns: None,
            tool_uses: 0,
            tokens: 0,
            duration_ms: 0,
            activity: None,
            linger_turns: 0,
        }];

        let lines = format_subagent_overlay(&agents, &opts);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("● Agents"));
        assert!(lines[1].contains("└─ ◦ 1 queued"));
    }
}
