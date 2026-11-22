//! The agent's task list. The conversation is its only store: the latest list is the last `todo`
//! result, or the `<todos>` block a compaction summary carries, so it follows the session through
//! resume and compaction without separate state.

use crate::args::{TodoItem, TodoStatus};
use crate::model::{ChatMessage, UserContent};
use std::fmt::Write as _;

pub const TODO_TOOL: &str = "todo";
const BLOCK_OPEN: &str = "<todos>";
const BLOCK_CLOSE: &str = "</todos>";

impl TodoStatus {
    fn marker(self) -> &'static str {
        match self {
            Self::Completed => "[x]",
            Self::InProgress => "[>]",
            Self::Pending => "[ ]",
        }
    }

    fn from_marker(marker: &str) -> Option<Self> {
        [Self::Completed, Self::InProgress, Self::Pending]
            .into_iter()
            .find(|status| status.marker() == marker)
    }
}

pub fn validate_todos(todos: &[TodoItem]) -> Result<(), String> {
    for (idx, todo) in todos.iter().enumerate() {
        if todo.content.trim().is_empty() {
            return Err(format!("todos[{idx}].content is empty"));
        }
        if todo.content.trim().contains(['\n', '\r']) {
            return Err(format!("todos[{idx}].content must be a single line"));
        }
    }
    let active: Vec<&str> = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::InProgress)
        .map(|todo| todo.content.trim())
        .collect();
    if active.len() > 1 {
        return Err(format!(
            "Only one todo may be in_progress, but {} are: {}. Keep the one you are working on in_progress and mark the others pending or completed.",
            active.len(),
            active.join("; ")
        ));
    }
    Ok(())
}

pub fn render_todos(todos: &[TodoItem]) -> String {
    let done = todos.iter().filter(|todo| todo.status == TodoStatus::Completed).count();
    let mut out = format!("{done}/{} done", todos.len());
    for todo in todos {
        let _ = write!(out, "\n{} {}", todo.status.marker(), todo.content.trim());
    }
    out
}

fn parse_item(line: &str) -> Option<TodoItem> {
    let (marker, content) = line.split_at_checked(3)?;
    Some(TodoItem {
        content: content.strip_prefix(' ')?.to_string(),
        status: TodoStatus::from_marker(marker)?,
    })
}

pub fn parse_todos(text: &str) -> Option<Vec<TodoItem>> {
    let mut lines = text.trim().lines();
    let (done, total) = lines.next()?.strip_suffix(" done")?.split_once('/')?;
    let (done, total): (usize, usize) = (done.parse().ok()?, total.parse().ok()?);
    let todos = lines
        .take_while(|line| !line.trim().is_empty())
        .map(parse_item)
        .collect::<Option<Vec<_>>>()?;
    let completed = todos.iter().filter(|todo| todo.status == TodoStatus::Completed).count();
    (todos.len() == total && completed == done).then_some(todos)
}

fn summary_todos(summary: &str) -> Option<Vec<TodoItem>> {
    let start = summary.rfind(BLOCK_OPEN)? + BLOCK_OPEN.len();
    let end = start + summary[start..].find(BLOCK_CLOSE)?;
    parse_todos(&summary[start..end])
}

fn message_todos(message: &ChatMessage) -> Option<Vec<TodoItem>> {
    match message {
        ChatMessage::System { content } => summary_todos(content),
        ChatMessage::User { content } => content.iter().rev().find_map(|item| match item {
            UserContent::ToolResult(result) if result.name == TODO_TOOL => parse_todos(&result.text()),
            _ => None,
        }),
        ChatMessage::Assistant { .. } => None,
    }
}

pub fn latest_todos(messages: &[ChatMessage]) -> Option<Vec<TodoItem>> {
    messages.iter().rev().find_map(message_todos)
}

/// The `<todos>` block a compaction summary appends so the model keeps its list: the latest list in
/// the folded messages, else the one `prior_summary` carried. Empty when the kept messages already
/// show a list or no item is unfinished.
pub fn carried_todos(prior_summary: Option<&str>, folded: &[ChatMessage], kept: &[ChatMessage]) -> String {
    if latest_todos(kept).is_some() {
        return String::new();
    }
    latest_todos(folded)
        .or_else(|| prior_summary.and_then(summary_todos))
        .filter(|todos| todos.iter().any(|todo| todo.status != TodoStatus::Completed))
        .map(|todos| format!("{BLOCK_OPEN}\n{}\n{BLOCK_CLOSE}", render_todos(&todos)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolResult;

    fn item(content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            content: content.to_string(),
            status,
        }
    }

    fn plan() -> Vec<TodoItem> {
        vec![
            item("Read the parser", TodoStatus::Completed),
            item("Fix the off-by-one", TodoStatus::InProgress),
            item("Add a regression test", TodoStatus::Pending),
        ]
    }

    fn result(name: &str, text: &str) -> ChatMessage {
        ChatMessage::User {
            content: vec![UserContent::ToolResult(ToolResult::new("call", name, text))],
        }
    }

    #[test]
    fn rendering_shows_progress_and_one_marker_per_item_and_parses_back() {
        let rendered = render_todos(&plan());

        assert_eq!(
            rendered,
            "1/3 done\n[x] Read the parser\n[>] Fix the off-by-one\n[ ] Add a regression test"
        );
        assert_eq!(parse_todos(&rendered), Some(plan()));
        assert_eq!(parse_todos(&render_todos(&[])), Some(Vec::new()));
        assert_eq!(parse_todos("Only one todo may be in_progress"), None);
        assert_eq!(parse_todos("2/3 done\n[x] a\n[ ] b\n[ ] c"), None);
        let with_notice = format!("{rendered}\n\n[context: 142k of 200k tokens used (71%); auto-compaction at 184k]");
        assert_eq!(
            parse_todos(&with_notice),
            Some(plan()),
            "a notice after the list is not part of it"
        );
    }

    #[test]
    fn validation_rejects_a_second_in_progress_item_and_blank_or_multiline_content() {
        let mut two_active = plan();
        two_active[2].status = TodoStatus::InProgress;

        assert_eq!(validate_todos(&plan()), Ok(()));
        let error = validate_todos(&two_active).unwrap_err();
        assert!(
            error.contains("Only one todo may be in_progress, but 2 are: Fix the off-by-one; Add a regression test"),
            "{error}"
        );
        assert!(validate_todos(&[item("  ", TodoStatus::Pending)]).is_err());
        assert!(validate_todos(&[item("a\nb", TodoStatus::Pending)]).is_err());
    }

    #[test]
    fn the_latest_list_is_the_last_todo_result_or_summary_block() {
        let older = vec![item("Old", TodoStatus::Pending)];
        let summary = format!("## Goal\nShip it\n\n<todos>\n{}\n</todos>", render_todos(&older));
        let history = vec![
            ChatMessage::system(summary),
            result(TODO_TOOL, &render_todos(&plan())),
            result(TODO_TOOL, "Invalid arguments for todo"),
            result("bash", &render_todos(&older)),
        ];

        assert_eq!(latest_todos(&history), Some(plan()));
        assert_eq!(latest_todos(&history[..1]), Some(older));
        assert_eq!(latest_todos(&history[2..]), None);
    }

    #[test]
    fn compaction_carries_unfinished_lists_the_kept_messages_do_not_show() {
        let folded = vec![result(TODO_TOOL, &render_todos(&plan()))];
        let block = carried_todos(None, &folded, &[]);
        let prior = format!("Earlier work\n\n{block}");
        let finished = vec![result(TODO_TOOL, &render_todos(&[item("Done", TodoStatus::Completed)]))];

        assert_eq!(block, format!("<todos>\n{}\n</todos>", render_todos(&plan())));
        assert_eq!(carried_todos(Some(&prior), &[], &[]), block);
        assert_eq!(carried_todos(None, &folded, &folded), "");
        assert_eq!(carried_todos(Some(&prior), &finished, &[]), "");
    }
}
