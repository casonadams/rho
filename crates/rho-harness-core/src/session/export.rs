//! Render a session branch as a shareable Markdown or HTML artifact.

use super::tree::SessionTree;
use rig::message::{AssistantContent, Message, UserContent};

enum Block {
    Text { role: &'static str, text: String },
    ToolCall { name: String },
}

fn push_user_content(content: &[UserContent], blocks: &mut Vec<Block>) {
    for item in content {
        match item {
            UserContent::Text(text) => blocks.push(Block::Text {
                role: "User",
                text: text.text.clone(),
            }),
            UserContent::ToolResult(result) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|part| part.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                blocks.push(Block::Text {
                    role: "Tool output",
                    text,
                });
            }
            _ => {}
        }
    }
}

fn push_assistant_content(content: &[AssistantContent], blocks: &mut Vec<Block>) {
    for item in content {
        match item {
            AssistantContent::Text(text) => blocks.push(Block::Text {
                role: "Assistant",
                text: text.text.clone(),
            }),
            AssistantContent::ToolCall(call) => blocks.push(Block::ToolCall {
                name: call.function.name.clone(),
            }),
            _ => {}
        }
    }
}

fn blocks(tree: &SessionTree) -> Vec<Block> {
    let mut blocks = Vec::new();
    for message in tree.active_messages() {
        match message {
            Message::System { .. } => {}
            Message::User { content } => push_user_content(&content, &mut blocks),
            Message::Assistant { content, .. } => push_assistant_content(&content, &mut blocks),
        }
    }
    blocks
}

fn session_title(tree: &SessionTree, session_id: &str) -> String {
    tree.session_name.clone().unwrap_or_else(|| session_id.to_string())
}

fn branch_context(tree: &SessionTree) -> (String, usize) {
    (
        tree.active_leaf_id.clone().unwrap_or_else(|| "root".to_string()),
        tree.len(),
    )
}

pub fn render_markdown(tree: &SessionTree, session_id: &str) -> String {
    let title = session_title(tree, session_id);
    let (leaf, count) = branch_context(tree);
    let mut out =
        format!("# rho session: {title}\n\n- Session: `{session_id}`\n- Branch: `{leaf}`\n- Messages: {count}\n");
    for block in blocks(tree) {
        match block {
            Block::Text { role, text } => {
                out.push_str(&format!("\n## {role}\n\n{text}\n"));
            }
            Block::ToolCall { name } => {
                out.push_str(&format!("\n*tool call: {name}*\n"));
            }
        }
    }
    out
}

fn render_html_block(block: Block) -> String {
    match block {
        Block::Text { role, text } => format!(
            "<section class=\"message {}\"><h2>{}</h2><p>{}</p></section>\n",
            role.to_lowercase(),
            escape_html(role),
            escape_html(&text)
        ),
        Block::ToolCall { name } => format!(
            "<p class=\"tool-call\"><em>tool call: {}</em></p>\n",
            escape_html(&name)
        ),
    }
}

const HTML_STYLE: &str = "<style>body{font-family:sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;color:#1a1a1a}\
h1{font-size:1.4rem}.meta{color:#666;font-size:.9rem}section.message{border-left:3px solid #ccc;\
padding-left:1rem;margin:1.5rem 0}section.user{border-color:#4a90d9}section.assistant{border-color:#7ab648}\
section.tool-output{border-color:#bbb}section.tool-output p,section.message p{white-space:pre-wrap}\
.tool-call{color:#666}</style>";

pub fn render_html(tree: &SessionTree, session_id: &str) -> String {
    let title = session_title(tree, session_id);
    let (leaf, count) = branch_context(tree);
    let mut body = String::new();
    for block in blocks(tree) {
        body.push_str(&render_html_block(block));
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>rho session: {}</title>\n\
         {}\n</head>\n<body>\n<h1>rho session: {}</h1>\n\
         <p class=\"meta\">Session <code>{}</code> \u{b7} Branch <code>{}</code> \u{b7} {} messages</p>\n{}\n</body>\n</html>\n",
        escape_html(&title),
        HTML_STYLE,
        escape_html(&title),
        escape_html(session_id),
        escape_html(&leaf),
        count,
        body
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests;
