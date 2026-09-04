use std::collections::BTreeSet;

use rig::message::{AssistantContent, Message};
use serde_json::Value;

use super::types::CompactionDetails;

pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    let stripped = trimmed.strip_prefix("./").unwrap_or(trimmed);
    stripped.to_string()
}

fn extract_path(arguments: &Value) -> Option<String> {
    if let Some(obj) = arguments.as_object() {
        obj.get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if let Some(s) = arguments.as_str() {
        if let Ok(val) = serde_json::from_str::<Value>(s) {
            return extract_path(&val);
        }
        None
    } else {
        None
    }
}

fn is_read_tool(name: &str) -> bool {
    name == "read" || name.ends_with(":read")
}

fn is_write_or_edit_tool(name: &str) -> bool {
    name == "write" || name.ends_with(":write") || name == "edit" || name.ends_with(":edit")
}

pub fn extract_file_ops(messages: &[Message], prior: Option<&CompactionDetails>) -> CompactionDetails {
    let mut read_set = BTreeSet::new();
    let mut modified_set = BTreeSet::new();

    if let Some(prior) = prior {
        for file in &prior.read_files {
            let norm = normalize_path(file);
            if !norm.is_empty() {
                read_set.insert(norm);
            }
        }
        for file in &prior.modified_files {
            let norm = normalize_path(file);
            if !norm.is_empty() {
                modified_set.insert(norm);
            }
        }
    }

    for msg in messages {
        if let Message::Assistant { content, .. } = msg {
            for item in content {
                if let AssistantContent::ToolCall(call) = item {
                    let name = call.function.name.as_str();
                    if let Some(raw_path) = extract_path(&call.function.arguments) {
                        let norm = normalize_path(&raw_path);
                        if norm.is_empty() {
                            continue;
                        }
                        if is_write_or_edit_tool(name) {
                            modified_set.insert(norm);
                        } else if is_read_tool(name) {
                            read_set.insert(norm);
                        }
                    }
                }
            }
        }
    }

    for file in &modified_set {
        read_set.remove(file);
    }

    CompactionDetails {
        read_files: read_set.into_iter().collect(),
        modified_files: modified_set.into_iter().collect(),
    }
}

pub fn render_file_lists_xml(details: &CompactionDetails) -> String {
    let mut blocks = Vec::new();

    if !details.read_files.is_empty() {
        let mut block = String::from("<read-files>\n");
        for file in &details.read_files {
            block.push_str(file);
            block.push('\n');
        }
        block.push_str("</read-files>");
        blocks.push(block);
    }

    if !details.modified_files.is_empty() {
        let mut block = String::from("<modified-files>\n");
        for file in &details.modified_files {
            block.push_str(file);
            block.push('\n');
        }
        block.push_str("</modified-files>");
        blocks.push(block);
    }

    blocks.join("\n\n")
}
