use std::collections::HashMap;

use rho_harness_core::tokens::cut_point::is_user_turn_start;
use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};

pub const DEFAULT_PRUNE_LINE_THRESHOLD: usize = 15;

fn is_assistant_final_response(message: &Message) -> bool {
    match message {
        Message::Assistant { content, .. } => {
            let has_tool_calls = content.iter().any(|c| matches!(c, AssistantContent::ToolCall(_)));
            let has_text = content
                .iter()
                .any(|c| matches!(c, AssistantContent::Text(t) if !t.text.trim().is_empty()));
            !has_tool_calls && has_text
        }
        _ => false,
    }
}

pub fn is_turn_boundary(message: &Message) -> bool {
    is_user_turn_start(message) || is_assistant_final_response(message)
}

pub fn find_turn_boundary_cutoff(messages: &[Message], volatile_turns: usize) -> usize {
    if volatile_turns == 0 || messages.is_empty() {
        return messages.len();
    }

    let mut remaining = volatile_turns;
    for (idx, msg) in messages.iter().enumerate().rev() {
        if is_turn_boundary(msg) {
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                return idx;
            }
        }
    }
    0
}

struct BashPruneDetails {
    lines: usize,
    size_str: String,
    log_path: Option<String>,
}

fn parse_bash_footer_details(text: &str) -> Option<BashPruneDetails> {
    let footer_start = text.rfind("[Command completed successfully with exit code 0 (")?;
    let footer = &text[footer_start..];
    let close = footer.find(']')?;
    let inner = &footer[..close];

    let lines_size_start = inner.find('(')?;
    let lines_size_end = inner.find(')')?;
    let parts = &inner[lines_size_start + 1..lines_size_end];
    let mut split = parts.split(',');
    let lines_part = split.next()?.trim();
    let size_part = split.next()?.trim();

    let lines = lines_part.split_whitespace().next()?.parse::<usize>().ok()?;

    let log_path = if let Some(path_start) = inner.find("Full log:") {
        let after = inner[path_start + "Full log:".len()..].trim();
        let path = after.split_whitespace().next()?.trim_matches(']');
        Some(path.to_string())
    } else {
        None
    };

    Some(BashPruneDetails {
        lines,
        size_str: size_part.to_string(),
        log_path,
    })
}

fn check_prunable_bash_output(text: &str, line_threshold: usize) -> Option<BashPruneDetails> {
    if text.contains("Command exited with code") || text.contains("Command timed out after") {
        return None;
    }

    if let Some(details) = parse_bash_footer_details(text) {
        if details.lines > line_threshold {
            return Some(details);
        }
        return None;
    }

    let line_count = text.lines().count();
    if line_count > line_threshold {
        Some(BashPruneDetails {
            lines: line_count,
            size_str: crate::tools::truncate::format_size(text.len()),
            log_path: None,
        })
    } else {
        None
    }
}

fn collect_tool_commands(messages: &[Message]) -> HashMap<String, String> {
    let mut tool_commands = HashMap::new();
    for msg in messages {
        if let Message::Assistant { content, .. } = msg {
            for item in content {
                if let AssistantContent::ToolCall(call) = item
                    && call.function.name == "bash"
                {
                    let cmd = call
                        .function
                        .arguments
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("bash")
                        .to_string();
                    tool_commands.insert(call.id.to_string(), cmd);
                }
            }
        }
    }
    tool_commands
}

fn format_pruned_stub(cmd: &str, details: &BashPruneDetails) -> String {
    match &details.log_path {
        Some(path) => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}). Full log: {path}]",
            details.lines, details.size_str
        ),
        None => format!(
            "[Command '{cmd}' completed with exit code 0. Output pruned ({} lines, {}).]",
            details.lines, details.size_str
        ),
    }
}

fn prune_tool_result_item(
    item: &UserContent,
    tool_commands: &HashMap<String, String>,
    line_threshold: usize,
) -> Option<UserContent> {
    let UserContent::ToolResult(res) = item else {
        return None;
    };
    let is_bash = res.name == "bash" || tool_commands.contains_key(&res.call.to_string());
    if !is_bash {
        return None;
    }
    let text = res
        .content
        .iter()
        .filter_map(|c| match c {
            ToolResultContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let details = check_prunable_bash_output(&text, line_threshold)?;
    let cmd = tool_commands
        .get(&res.call.to_string())
        .cloned()
        .unwrap_or_else(|| "bash".to_string());
    let stub = format_pruned_stub(&cmd, &details);
    Some(UserContent::ToolResult(rig::message::ToolResult {
        call: res.call.clone(),
        provider: res.provider.clone(),
        name: res.name.clone(),
        content: vec![ToolResultContent::text(stub)],
    }))
}

fn prune_user_message_content(
    content: &[UserContent],
    tool_commands: &HashMap<String, String>,
    line_threshold: usize,
) -> Option<Vec<UserContent>> {
    let mut modified = false;
    let new_content: Vec<UserContent> = content
        .iter()
        .map(
            |item| match prune_tool_result_item(item, tool_commands, line_threshold) {
                Some(pruned) => {
                    modified = true;
                    pruned
                }
                None => item.clone(),
            },
        )
        .collect();

    if modified { Some(new_content) } else { None }
}

pub fn prune_historical_tool_outputs(
    messages: &[Message],
    volatile_turns: usize,
    line_threshold: usize,
) -> Vec<Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    let cutoff_idx = find_turn_boundary_cutoff(messages, volatile_turns);
    if cutoff_idx == 0 {
        return messages.to_vec();
    }

    let tool_commands = collect_tool_commands(messages);
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            if idx >= cutoff_idx {
                return msg.clone();
            }
            let Message::User { content } = msg else {
                return msg.clone();
            };
            match prune_user_message_content(content, &tool_commands, line_threshold) {
                Some(new_content) => Message::User { content: new_content },
                None => msg.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::message::{AssistantContent, Text, ToolCall, ToolCallId, ToolFunction};

    fn make_tool_turn(cid: &str, tool: &str, cmd: &str, tool_output: &str) -> (Message, Message) {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(tool.to_string(), serde_json::json!({ "command": cmd })),
        );
        let res = rig::message::ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: tool.to_string(),
            content: vec![ToolResultContent::Text(Text::new(tool_output))],
        };
        (
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
        )
    }

    #[test]
    fn test_current_turn_bash_output_is_not_pruned() {
        let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let output_with_footer = format!(
            "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
        );
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

        let history = vec![Message::user("Please test"), call_msg, res_msg];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned.len(), 3);

        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("line 1"));
        assert!(text.contains("line 30"));
    }

    #[test]
    fn test_prior_turn_successful_verbose_bash_output_is_pruned() {
        let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let output_with_footer = format!(
            "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
        );
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

        let history = vec![
            Message::user("Please test"),
            call_msg,
            res_msg,
            Message::assistant("Tests passed successfully!"),
            Message::user("Now run clippy"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert_eq!(pruned.len(), 5);

        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(
            text,
            "[Command 'cargo test' completed with exit code 0. Output pruned (30 lines, 300B). Full log: /tmp/log.txt]"
        );
    }

    #[test]
    fn test_prior_turn_failed_bash_output_is_not_pruned() {
        let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
        let output_with_err = format!("{output}\n\nCommand exited with code 1");
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_err);

        let history = vec![
            Message::user("Please test"),
            call_msg,
            res_msg,
            Message::assistant("Tests failed with exit code 1"),
            Message::user("Fix the tests"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert!(text.contains("error 1"));
        assert!(text.contains("Command exited with code 1"));
    }

    #[test]
    fn test_prior_turn_short_bash_output_is_not_pruned() {
        let output = "line 1\nline 2\nline 3\n";
        let (call_msg, res_msg) = make_tool_turn("c1", "bash", "git status", output);

        let history = vec![
            Message::user("Status"),
            call_msg,
            res_msg,
            Message::assistant("Clean working tree"),
            Message::user("Continue"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, output);
    }

    #[test]
    fn test_prior_turn_non_bash_tool_output_is_not_pruned() {
        let output = (1..=40)
            .map(|i| format!("fn line_{i}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (call_msg, res_msg) = make_tool_turn("c1", "read", "src/lib.rs", &output);

        let history = vec![
            Message::user("Read file"),
            call_msg,
            res_msg,
            Message::assistant("Read file done"),
            Message::user("Next"),
        ];

        let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        let Message::User { content } = &pruned[2] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        let text = match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        };
        assert_eq!(text, &output);
    }

    fn extract_tool_result_first_text(message: &Message) -> &str {
        let Message::User { content } = message else { panic!() };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        match &res.content[0] {
            ToolResultContent::Text(t) => &t.text,
            _ => panic!(),
        }
    }

    #[test]
    fn test_multi_turn_sequence_prunes_completed_verbose_outputs_and_preserves_failures() {
        let out_verbose = (1..=25)
            .map(|i| format!("test {i} passed"))
            .collect::<Vec<_>>()
            .join("\n");
        let out_verbose_footer = format!(
            "{out_verbose}\n\n[Command completed successfully with exit code 0 (25 lines, 400B). Full log: /tmp/test.log]"
        );
        let (call1, res1) = make_tool_turn("c1", "bash", "cargo test", &out_verbose_footer);
        let out_fail = "error[E0425]: cannot find value `x` in this scope\n\nCommand exited with code 1";
        let (call2, res2) = make_tool_turn("c2", "bash", "cargo check", out_fail);
        let (call3, res3) = make_tool_turn("c3", "bash", "cargo build", &out_verbose_footer);

        let t1_history = vec![Message::user("Run test"), call1.clone(), res1.clone()];
        assert_eq!(
            prune_historical_tool_outputs(&t1_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD),
            t1_history
        );

        let t2_history = vec![
            Message::user("Run test"),
            call1,
            res1,
            Message::assistant("Tests passed"),
            Message::user("Now check"),
            call2.clone(),
            res2.clone(),
        ];
        let t2_pruned = prune_historical_tool_outputs(&t2_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert!(extract_tool_result_first_text(&t2_pruned[2]).contains("Output pruned (25 lines, 400B)"));
        assert!(extract_tool_result_first_text(&t2_pruned[6]).contains("cannot find value `x`"));

        let t3_history = vec![
            t2_pruned[0].clone(),
            t2_pruned[1].clone(),
            t2_pruned[2].clone(),
            t2_pruned[3].clone(),
            t2_pruned[4].clone(),
            call2,
            res2,
            Message::assistant("Check failed with error E0425"),
            Message::user("Now build"),
            call3,
            res3,
        ];
        let t3_pruned = prune_historical_tool_outputs(&t3_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
        assert!(extract_tool_result_first_text(&t3_pruned[2]).contains("Output pruned (25 lines, 400B)"));
        assert!(extract_tool_result_first_text(&t3_pruned[6]).contains("cannot find value `x`"));
        assert!(extract_tool_result_first_text(&t3_pruned[10]).contains("test 1 passed"));
    }
}
