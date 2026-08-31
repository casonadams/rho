use crate::ui::interactive::footer::truncate_to_width;
use crate::ui::theme::Theme;
use anstyle::{AnsiColor, Color, Effects, Style};
use rho_plugin_builtin::{TaskStatus, TodoTask};

pub const DEFAULT_MAX_TODO_LINES: usize = 12;

#[derive(Debug, Clone, Copy)]
pub struct TodoOverlayOptions<'a> {
    pub theme: &'a Theme,
    pub width: usize,
    pub max_lines: usize,
    pub expanded: bool,
}

impl<'a> TodoOverlayOptions<'a> {
    pub fn new(theme: &'a Theme, width: usize) -> Self {
        Self {
            theme,
            width,
            max_lines: DEFAULT_MAX_TODO_LINES,
            expanded: false,
        }
    }

    pub fn with_limits(mut self, max_lines: usize, expanded: bool) -> Self {
        self.max_lines = max_lines;
        self.expanded = expanded;
        self
    }
}

/// Returns the status glyph styled with theme colors for the overlay tree.
pub fn overlay_status_glyph(status: TaskStatus, theme: &Theme) -> String {
    match status {
        TaskStatus::Pending => {
            let dim = theme.dimmed;
            format!("{dim}○{dim:#}")
        }
        TaskStatus::InProgress => {
            let warn = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
            format!("{warn}◐{warn:#}")
        }
        TaskStatus::Completed => {
            let ok = theme.tool_ok;
            format!("{ok}✓{ok:#}")
        }
        TaskStatus::Deleted => {
            let err = theme.tool_err;
            format!("{err}✗{err:#}")
        }
    }
}

/// Formats a single Todo task row with tree prefix, glyph, ID, subject, activeForm, and blockedBy.
pub fn format_todo_row(task: &TodoTask, prefix: &str, opts: &TodoOverlayOptions<'_>) -> String {
    let theme = opts.theme;
    let dim = theme.dimmed;
    let glyph = overlay_status_glyph(task.status, theme);
    let id_str = format!("{dim}#{}{dim:#}", task.id);

    let subject_str = match task.status {
        TaskStatus::InProgress => {
            let accent = theme.highlight;
            format!("{accent}{}{accent:#}", task.subject)
        }
        TaskStatus::Completed | TaskStatus::Deleted => {
            let strike_dim = Style::new().effects(Effects::STRIKETHROUGH | Effects::DIMMED);
            format!("{strike_dim}{}{strike_dim:#}", task.subject)
        }
        TaskStatus::Pending => task.subject.clone(),
    };

    let mut line = format!("{dim}{prefix}{dim:#} {glyph} {id_str} {subject_str}");

    if task.status == TaskStatus::InProgress
        && let Some(active_form) = &task.active_form
        && !active_form.is_empty()
    {
        line.push_str(&format!(" {dim}({active_form}){dim:#}"));
    }

    if !task.blocked_by.is_empty() {
        let blocked_ids = task
            .blocked_by
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        line.push_str(&format!(" {dim}⛓ {blocked_ids}{dim:#}"));
    }

    truncate_to_width(&line, opts.width)
}

/// Formats the complete Todo overlay tree block.
pub fn format_todo_overlay(tasks: &[TodoTask], opts: &TodoOverlayOptions<'_>) -> Vec<String> {
    let non_deleted: Vec<&TodoTask> = tasks.iter().filter(|t| t.status != TaskStatus::Deleted).collect();
    if non_deleted.is_empty() {
        return Vec::new();
    }

    let total = non_deleted.len();
    let completed = non_deleted.iter().filter(|t| t.status == TaskStatus::Completed).count();
    let has_active = non_deleted
        .iter()
        .any(|t| t.status == TaskStatus::InProgress || t.status == TaskStatus::Pending);

    let (heading_icon, heading_color) = if has_active {
        ("●", opts.theme.highlight)
    } else {
        ("○", opts.theme.dimmed)
    };

    let heading_text = format!("Todos ({completed}/{total})");
    let heading_line = truncate_to_width(
        &format!("{heading_color}{heading_icon} {heading_text}{heading_color:#}"),
        opts.width,
    );

    let mut lines = vec![heading_line];
    let max_body_lines = if opts.expanded {
        total
    } else {
        opts.max_lines.saturating_sub(1).max(1)
    };

    if total <= max_body_lines {
        for (i, task) in non_deleted.iter().enumerate() {
            let is_last = i + 1 == total;
            let prefix = if is_last { "└─" } else { "├─" };
            lines.push(format_todo_row(task, prefix, opts));
        }
    } else {
        let budget = max_body_lines.saturating_sub(1).max(1);

        let unfinished: Vec<&TodoTask> = non_deleted
            .iter()
            .copied()
            .filter(|t| t.status != TaskStatus::Completed)
            .collect();
        let completed_tasks: Vec<&TodoTask> = non_deleted
            .iter()
            .copied()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();

        let mut visible = Vec::new();
        let hidden_completed;
        let hidden_pending;

        if unfinished.len() <= budget {
            let remaining_budget = budget - unfinished.len();
            let take_completed = completed_tasks.len().min(remaining_budget);
            hidden_completed = completed_tasks.len() - take_completed;
            hidden_pending = 0;

            let mut taken_completed_set = std::collections::HashSet::new();
            for t in completed_tasks.iter().take(take_completed) {
                taken_completed_set.insert(t.id);
            }

            for t in &non_deleted {
                if t.status != TaskStatus::Completed || taken_completed_set.contains(&t.id) {
                    visible.push(*t);
                }
            }
        } else {
            hidden_completed = completed_tasks.len();
            for t in unfinished.iter().take(budget) {
                visible.push(*t);
            }
            hidden_pending = unfinished.len() - budget;
        }

        for task in &visible {
            lines.push(format_todo_row(task, "├─", opts));
        }

        let dim = opts.theme.dimmed;
        let total_hidden = hidden_completed + hidden_pending;
        let mut parts = Vec::new();
        if hidden_completed > 0 {
            parts.push(format!("{hidden_completed} completed"));
        }
        if hidden_pending > 0 {
            parts.push(format!("{hidden_pending} pending"));
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
    fn test_empty_tasks_returns_empty() {
        let theme = Theme::default();
        let opts = TodoOverlayOptions::new(&theme, 80);
        let lines = format_todo_overlay(&[], &opts);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_format_todo_overlay_layout() {
        let theme = Theme::default();
        let opts = TodoOverlayOptions::new(&theme, 80);
        let tasks = vec![
            TodoTask {
                id: 1,
                subject: "Add ProviderId::Antigravity to rho-core".to_string(),
                description: None,
                status: TaskStatus::Completed,
                active_form: None,
                owner: None,
                blocked_by: vec![],
                metadata: None,
            },
            TodoTask {
                id: 2,
                subject: "Implement Antigravity OAuth login and refresh".to_string(),
                description: None,
                status: TaskStatus::Completed,
                active_form: None,
                owner: None,
                blocked_by: vec![],
                metadata: None,
            },
            TodoTask {
                id: 3,
                subject: "Implement streaming client".to_string(),
                description: None,
                status: TaskStatus::Completed,
                active_form: None,
                owner: None,
                blocked_by: vec![2],
                metadata: None,
            },
            TodoTask {
                id: 4,
                subject: "Wire Antigravity in registry".to_string(),
                description: None,
                status: TaskStatus::InProgress,
                active_form: Some("wiring Antigravity".to_string()),
                owner: None,
                blocked_by: vec![],
                metadata: None,
            },
            TodoTask {
                id: 5,
                subject: "Verify tests and clippy".to_string(),
                description: None,
                status: TaskStatus::Pending,
                active_form: None,
                owner: None,
                blocked_by: vec![4],
                metadata: None,
            },
        ];

        let lines = format_todo_overlay(&tasks, &opts);
        assert_eq!(lines.len(), 6);
        assert!(lines[0].contains("Todos (3/5)"));
        assert!(lines[1].contains("├─"));
        assert!(lines[1].contains("✓"));
        assert!(lines[1].contains("#1"));
        assert!(lines[3].contains("⛓ #2"));
        assert!(lines[4].contains("◐"));
        assert!(lines[4].contains("(wiring Antigravity)"));
        assert!(lines[5].contains("└─"));
        assert!(lines[5].contains("○"));
        assert!(lines[5].contains("#5"));
        assert!(lines[5].contains("⛓ #4"));
    }

    #[test]
    fn test_all_completed_heading_dimmed() {
        let theme = Theme::default();
        let opts = TodoOverlayOptions::new(&theme, 80);
        let tasks = vec![TodoTask {
            id: 1,
            subject: "Task 1".to_string(),
            description: None,
            status: TaskStatus::Completed,
            active_form: None,
            owner: None,
            blocked_by: vec![],
            metadata: None,
        }];

        let lines = format_todo_overlay(&tasks, &opts);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("○ Todos (1/1)"));
        assert!(lines[1].contains("└─"));
    }

    #[test]
    fn test_overflow_summary() {
        let theme = Theme::default();
        let opts = TodoOverlayOptions::new(&theme, 80).with_limits(5, false);
        let mut tasks = Vec::new();
        for i in 1..=10 {
            tasks.push(TodoTask {
                id: i,
                subject: format!("Task {i}"),
                description: None,
                status: if i <= 5 {
                    TaskStatus::Completed
                } else {
                    TaskStatus::Pending
                },
                active_form: None,
                owner: None,
                blocked_by: vec![],
                metadata: None,
            });
        }

        let lines = format_todo_overlay(&tasks, &opts);
        assert_eq!(lines.len(), 5);
        assert!(lines[4].contains("+7 more"));
        assert!(lines[4].contains("5 completed"));
        assert!(lines[4].contains("2 pending"));
    }
}
