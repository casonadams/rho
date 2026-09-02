use super::*;
use crate::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use chrono::Utc;
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};

fn tree_with_conversation() -> SessionTree {
    let mut tree = SessionTree::new();
    tree.set_session_name("export demo".to_string());
    tree.set_active_leaf(Some("leaf-1".to_string()));
    let node = TreeNodeData {
        id: "node-1".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            Message::user("what is <html> & \"quotes\"?"),
            Message::assistant("it is escaped"),
        ],
        label: None,
        metadata: None,
    };
    tree.add_node(node);
    let tool_node = TreeNodeData {
        id: "node-2".to_string(),
        parent_id: Some("node-1".to_string()),
        timestamp: Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new("call-1").unwrap(),
                    ToolFunction::new("bash".to_string(), serde_json::json!({"command": "ls"})),
                ))],
            },
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: ToolCallId::new("call-1").unwrap(),
                    provider: None,
                    name: "bash".to_string(),
                    content: vec![ToolResultContent::text("file-a\nfile-b")],
                })],
            },
        ],
        label: None,
        metadata: None,
    };
    tree.add_node(tool_node);
    tree
}

#[test]
fn markdown_render_includes_header_roles_and_branch_context() {
    let tree = tree_with_conversation();
    let markdown = render_markdown(&tree, "session-123");

    assert!(markdown.contains("# rho session: export demo"), "{markdown}");
    assert!(markdown.contains("- Session: `session-123`"), "{markdown}");
    assert!(markdown.contains("- Branch: `node-2`"), "{markdown}");
    assert!(markdown.contains("## User"), "{markdown}");
    assert!(markdown.contains("## Assistant"), "{markdown}");
    assert!(markdown.contains("## Tool output"), "{markdown}");
    assert!(markdown.contains("*tool call: bash*"), "{markdown}");
}

#[test]
fn html_render_escapes_markup_and_includes_metadata() {
    let tree = tree_with_conversation();
    let html = render_html(&tree, "session-1");

    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(html.contains("rho session: export demo"), "{html}");
    assert!(html.contains("&lt;html&gt; &amp; &quot;quotes&quot;?"), "{html}");
    assert!(!html.contains("<html> &"), "{html}");
    assert!(html.contains("Branch <code>node-2</code>"), "{html}");
    assert!(html.contains("tool call: bash"), "{html}");
}

#[test]
fn empty_tree_renders_header_without_messages() {
    let tree = SessionTree::new();
    let markdown = render_markdown(&tree, "session-empty");
    assert!(markdown.contains("# rho session: session-empty"), "{markdown}");
    assert!(markdown.contains("- Branch: `root`"), "{markdown}");
    assert!(!markdown.contains("## User"), "{markdown}");

    let html = render_html(&tree, "session-empty");
    assert!(html.contains("Branch <code>root</code>"), "{html}");
}

#[test]
fn falls_back_to_session_id_when_unnamed() {
    let mut tree = SessionTree::new();
    tree.set_active_leaf(Some("leaf-1".to_string()));
    tree.add_node(TreeNodeData {
        id: "node-1".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![Message::user("hello")],
        label: None,
        metadata: None,
    });
    let markdown = render_markdown(&tree, "abc-123");
    assert!(markdown.contains("# rho session: abc-123"), "{markdown}");
}
