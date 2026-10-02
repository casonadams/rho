use super::*;
use crate::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, ToolResult, UserContent};
use crate::session::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use chrono::Utc;

fn sample_tool_node() -> TreeNodeData {
    TreeNodeData {
        id: "node-2".to_string(),
        parent_id: Some("node-1".to_string()),
        timestamp: Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![
            ChatMessage::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    "call-1",
                    ToolFunction::new("bash", serde_json::json!({"command": "ls"})),
                ))],
            },
            ChatMessage::User {
                content: vec![UserContent::ToolResult(ToolResult::new(
                    "call-1",
                    "bash",
                    "file-a\nfile-b",
                ))],
            },
        ],
        label: None,
        metadata: None,
    }
}

fn tree_with_conversation() -> SessionTree {
    let mut tree = SessionTree::new();
    tree.set_session_name("export demo".to_string());
    tree.set_active_leaf(Some("leaf-1".to_string()));
    tree.add_node(TreeNodeData {
        id: "node-1".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![
            ChatMessage::user("what is <html> & \"quotes\"?"),
            ChatMessage::assistant("it is escaped"),
        ],
        label: None,
        metadata: None,
    });
    tree.add_node(sample_tool_node());
    tree
}

#[test]
fn markdown_render_includes_header_roles_and_branch_context() {
    let tree = tree_with_conversation();
    let markdown = render_markdown(&tree, "session-123");
    let fragments = [
        "# rho session: export demo",
        "- Session: `session-123`",
        "- Branch: `node-2`",
        "## User",
        "## Assistant",
        "## Tool output",
        "*tool call: bash*",
    ];
    for f in fragments {
        assert!(markdown.contains(f), "{markdown}");
    }
}

#[test]
fn html_render_escapes_markup_and_includes_metadata() {
    let tree = tree_with_conversation();
    let html = render_html(&tree, "session-1");
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(!html.contains("<html> &"), "{html}");
    let fragments = [
        "rho session: export demo",
        "&lt;html&gt; &amp; &quot;quotes&quot;?",
        "Branch <code>node-2</code>",
        "tool call: bash",
    ];
    for f in fragments {
        assert!(html.contains(f), "{html}");
    }
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
        messages: vec![ChatMessage::user("hello")],
        label: None,
        metadata: None,
    });
    let markdown = render_markdown(&tree, "abc-123");
    assert!(markdown.contains("# rho session: abc-123"), "{markdown}");
}
