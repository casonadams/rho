//! Reads what a session listing shows without replaying the session: records are probed for
//! their type and tree links, and message payloads are parsed only for the first root node.

use super::format::{parse_header, validate_header};
use super::fs::session_error;
use super::tree::TitleSource;
use crate::error::Result;
use crate::model::{ChatMessage, UserContent};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::io::BufRead;
use std::path::Path;

#[derive(Deserialize)]
struct RecordProbe {
    record_type: String,
    #[serde(default)]
    node: Option<NodeProbe>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    source: TitleSource,
    #[serde(default)]
    active_leaf_id: Option<String>,
}

#[derive(Deserialize)]
struct NodeProbe {
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct Messages {
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct NodeMessages {
    node: Messages,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ScannedSession {
    pub(crate) node_count: usize,
    pub(crate) name: Option<String>,
    name_source: Option<TitleSource>,
    has_active_leaf: bool,
    /// The first root node's first user text; `Some(None)` once a root without one was seen.
    pub(crate) root_text: Option<Option<String>>,
}

fn first_user_text(messages: Vec<ChatMessage>) -> Option<String> {
    messages.into_iter().find_map(|message| match message {
        ChatMessage::User { content } => match content.into_iter().next() {
            Some(UserContent::Text(text)) => Some(text.text),
            _ => None,
        },
        _ => None,
    })
}

fn malformed() -> crate::error::AppError {
    session_error("session contains a malformed committed record")
}

impl ScannedSession {
    fn add_node(&mut self, is_root: bool, messages: impl FnOnce() -> Result<Vec<ChatMessage>>) -> Result<()> {
        self.node_count += 1;
        self.has_active_leaf = true;
        if is_root && self.root_text.is_none() {
            self.root_text = Some(first_user_text(messages()?));
        }
        Ok(())
    }

    fn apply(&mut self, probe: RecordProbe, line: &[u8]) -> Result<()> {
        let top_messages = || {
            Ok(serde_json::from_slice::<Messages>(line)
                .map_err(|_| malformed())?
                .messages)
        };
        match probe.record_type.as_str() {
            "canonical_messages" | "checkpoint_promoted" => self.add_node(!self.has_active_leaf, top_messages)?,
            "tree_node" => {
                let is_root = probe.node.ok_or_else(malformed)?.parent_id.is_none();
                self.add_node(is_root, || {
                    Ok(serde_json::from_slice::<NodeMessages>(line)
                        .map_err(|_| malformed())?
                        .node
                        .messages)
                })?;
            }
            "active_leaf_changed" => self.has_active_leaf = probe.active_leaf_id.is_some(),
            "canonical_reset" => *self = Self::default(),
            "session_named" => self.apply_name(probe.name.ok_or_else(malformed)?, probe.source),
            _ => {}
        }
        Ok(())
    }

    fn apply_name(&mut self, name: String, source: TitleSource) {
        if source != TitleSource::Auto || self.name_source != Some(TitleSource::User) {
            self.name = Some(name);
            self.name_source = Some(source);
        }
    }
}

/// Scans the committed records of the session file at `path`; a torn trailing line is ignored.
pub(crate) fn scan_session(path: &Path, expected_id: &str) -> Result<(DateTime<Utc>, ScannedSession)> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut line = Vec::new();
    if reader.read_until(b'\n', &mut line)? == 0 || line.pop() != Some(b'\n') {
        return Err(session_error("session is missing the mandatory version header"));
    }
    let header = parse_header(&line)?;
    validate_header(&header, expected_id)?;
    let mut scanned = ScannedSession::default();
    loop {
        line.clear();
        reader.read_until(b'\n', &mut line)?;
        if line.pop() != Some(b'\n') {
            return Ok((header.created_at, scanned));
        }
        let probe: RecordProbe = serde_json::from_slice(&line).map_err(|_| malformed())?;
        scanned.apply(probe, &line)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionManager, load_file};

    async fn scan(manager: &SessionManager) -> ScannedSession {
        scan_session(&manager.file_path, &manager.session_id).unwrap().1
    }

    #[tokio::test]
    async fn scan_matches_the_replayed_tree_across_names_branches_and_resets() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new_async(&dir, None).await.unwrap();
        let sid = session.session_id.clone();
        let turn = |n: usize| {
            vec![
                ChatMessage::user(format!("q{n}")),
                ChatMessage::assistant(format!("a{n}")),
            ]
        };
        session.append_messages(&sid, turn(1)).await.unwrap();
        session.set_session_name("User Name").await.unwrap();
        assert!(!session.set_auto_session_name("Auto Name").await.unwrap());
        session.append_messages(&sid, turn(2)).await.unwrap();
        session.switch_branch(None).await.unwrap();
        session.append_branch_summary("side quest", "elsewhere").await.unwrap();
        session.append_messages(&sid, turn(3)).await.unwrap();

        let scanned = scan(&session).await;
        let replayed = load_file(&session.file_path, &sid).unwrap();

        assert_eq!(scanned.node_count, replayed.tree.len());
        assert_eq!(scanned.name, replayed.tree.session_name);
        assert_eq!(scanned.name.as_deref(), Some("User Name"));
        assert_eq!(scanned.root_text, Some(Some("q1".to_string())));
    }

    #[test]
    fn scan_parses_messages_only_for_the_first_root_node() {
        let dir = crate::test_support::temp_dir();
        let id = "scan-payloads";
        let header = format!(
            r#"{{"record_type":"header","version":2,"session_id":"{id}","created_at":"2020-01-02T03:04:05Z"}}"#
        );
        let root = r#"{"record_type":"tree_node","node":{"id":"r","parent_id":null,"messages":[{"role":"user","content":[{"type":"text","text":"first"}]}]}}"#;
        let child = r#"{"record_type":"tree_node","node":{"id":"c","parent_id":"r","messages":"not messages"}}"#;
        let event = r#"{"record_type":"audit_event","event":{"payload":{"anything":[1,2,3]}}}"#;
        let named = r#"{"record_type":"session_named","name":"Named"}"#;
        let path = dir.join(format!("{id}.jsonl"));
        std::fs::write(&path, format!("{header}\n{root}\n{child}\n{event}\n{named}\n{{\"torn")).unwrap();

        let (created_at, scanned) = scan_session(&path, id).unwrap();

        assert_eq!(created_at.to_rfc3339(), "2020-01-02T03:04:05+00:00");
        assert_eq!(scanned.node_count, 2);
        assert_eq!(scanned.name.as_deref(), Some("Named"));
        assert_eq!(scanned.root_text, Some(Some("first".to_string())));
    }

    #[test]
    fn scan_rejects_headerless_and_malformed_files() {
        let dir = crate::test_support::temp_dir();
        let header = r#"{"record_type":"header","version":2,"session_id":"bad","created_at":"2020-01-02T03:04:05Z"}"#;
        let cases = [
            ("", "mandatory version header"),
            ("not a header\n", "legacy session"),
            (&*format!("{header}\n{{malformed}}\n"), "malformed committed record"),
        ];
        for (body, expected) in cases {
            let path = dir.join("bad.jsonl");
            std::fs::write(&path, body).unwrap();
            let error = scan_session(&path, "bad").unwrap_err().to_string();
            assert!(error.contains(expected), "{body:?}: {error}");
        }
    }
}
