//! Fits a compaction summary and its carried todo block into the configured byte limit.

const TODOS_CLOSE: &str = "</todos>";
const XML_BLOCKS: [&str; 2] = ["read-files", "modified-files"];

/// `summary` followed by `todos`, within `max_bytes` (0 means no limit). The todo block gets at most
/// half the limit, cut by whole lines, and the summary never ends inside a tag.
pub(super) fn fit_summary(summary: &str, todos: &str, max_bytes: usize) -> String {
    let (summary, todos) = if max_bytes == 0 {
        (summary, todos.to_string())
    } else {
        let todos = cap_todos(todos, max_bytes / 2);
        let reserved = if todos.is_empty() { 0 } else { todos.len() + 2 };
        (cut_summary(summary, max_bytes.saturating_sub(reserved)), todos)
    };
    if todos.is_empty() {
        summary.to_string()
    } else if summary.is_empty() {
        todos
    } else {
        format!("{summary}\n\n{todos}")
    }
}

fn cap_todos(todos: &str, cap: usize) -> String {
    if todos.len() <= cap {
        return todos.to_string();
    }
    let mut lines = todos.lines();
    let mut block = lines.next().unwrap_or_default().to_string();
    for line in lines.filter(|line| *line != TODOS_CLOSE) {
        if block.len() + line.len() + TODOS_CLOSE.len() + 2 > cap {
            break;
        }
        block.push('\n');
        block.push_str(line);
    }
    block.push('\n');
    block.push_str(TODOS_CLOSE);
    if block.len() > cap { String::new() } else { block }
}

fn cut_summary(summary: &str, budget: usize) -> &str {
    if summary.len() <= budget {
        return summary;
    }
    let mut head = &summary[..summary.floor_char_boundary(budget)];
    if let Some(open) = head.rfind('<')
        && !head[open..].contains('>')
    {
        head = &head[..open];
    }
    let unclosed = XML_BLOCKS
        .iter()
        .filter_map(|tag| {
            let open = head.rfind(&format!("<{tag}>"))?;
            (!head[open..].contains(&format!("</{tag}>"))).then_some(open)
        })
        .min();
    head[..unclosed.unwrap_or(head.len())].trim_end()
}

#[cfg(test)]
mod tests {
    use super::fit_summary;

    #[test]
    fn the_summary_gives_way_so_the_todo_block_stays_whole_within_the_limit() {
        let todos = "<todos>\n0/1 done\n[>] Ship\n</todos>";

        let fitted = fit_summary(&"s".repeat(100), todos, 80);

        assert_eq!(fitted.len(), 80);
        assert!(fitted.ends_with(&format!("\n\n{todos}")), "{fitted}");
        assert_eq!(fit_summary("summary", "", 3), "sum");
        assert_eq!(fit_summary("summary", todos, 0), format!("summary\n\n{todos}"));
    }

    #[test]
    fn an_oversized_todo_block_is_capped_by_whole_lines_and_the_summary_keeps_its_share() {
        let items: String = (0..50).map(|i| format!("[ ] Item number {i}\n")).collect();
        let todos = format!("<todos>\n{items}</todos>");

        let fitted = fit_summary(&"s".repeat(500), &todos, 200);

        assert!(fitted.len() <= 200, "{} bytes", fitted.len());
        assert!(
            fitted.starts_with(&"s".repeat(90)),
            "the summary keeps a real share: {fitted}"
        );
        assert!(fitted.ends_with("</todos>"), "{fitted}");
        let block = &fitted[fitted.find("<todos>").unwrap()..];
        assert!(
            block
                .lines()
                .skip(1)
                .all(|line| line.starts_with("[ ] Item number") || line == "</todos>")
        );
    }

    #[test]
    fn the_summary_is_never_cut_inside_a_tag() {
        let files = "<read-files>\na.rs\nb.rs\n</read-files>\n\n<modified-files>\nc.rs\n</modified-files>";
        let summary = format!("Did things.\n\n{files}");

        for max in 1..=summary.len() {
            let fitted = fit_summary(&summary, "", max);
            assert!(fitted.len() <= max, "{max}: {fitted:?}");
            for tag in ["read-files", "modified-files"] {
                let opens = fitted.matches(&format!("<{tag}>")).count();
                assert_eq!(opens, fitted.matches(&format!("</{tag}>")).count(), "{max}: {fitted:?}");
            }
            let last_open = fitted.rfind('<');
            assert!(last_open.is_none_or(|i| fitted[i..].contains('>')), "{max}: {fitted:?}");
        }
    }
}
