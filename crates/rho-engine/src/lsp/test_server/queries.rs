//! The fake server's navigation, rename, code action, formatting, and symbol answers.

use super::{FakeServer, range, word_at};
use serde_json::{Value, json};

impl FakeServer {
    pub(super) fn occurrences(&self, word: &str) -> Vec<Value> {
        let mut found = Vec::new();
        let mut uris: Vec<&String> = self.docs.keys().collect();
        uris.sort();
        for uri in uris {
            for (index, line) in self.text(uri).lines().enumerate() {
                for (col, _) in line.match_indices(word) {
                    if word_at(line, 0, col as u64) == word {
                        found.push(json!({ "uri": uri, "range": range(index, col, index, col + word.len()) }));
                    }
                }
            }
        }
        found
    }

    pub(super) fn definition(&self, params: &Value) -> Value {
        let word = self.word(params);
        let definitions: Vec<Value> = self
            .occurrences(&word)
            .into_iter()
            .filter(|location| {
                let uri = location["uri"].as_str().unwrap_or("");
                let line = location["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
                let text = self.text(uri).lines().nth(line).unwrap_or("");
                text.contains(&format!("fn {word}")) || text.contains(&format!("struct {word}"))
            })
            .collect();
        Value::Array(definitions)
    }

    pub(super) fn rename(&self, params: &Value) -> Value {
        let new_name = params["newName"].as_str().unwrap_or("");
        let mut changes = serde_json::Map::new();
        for location in self.occurrences(&self.word(params)) {
            let uri = location["uri"].as_str().unwrap_or("").to_string();
            let edit = json!({ "range": location["range"], "newText": new_name });
            if let Some(list) = changes.entry(uri).or_insert_with(|| json!([])).as_array_mut() {
                list.push(edit);
            }
        }
        json!({ "changes": changes })
    }

    pub(super) fn code_actions(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let first = params["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
        let last = params["range"]["end"]["line"].as_u64().unwrap_or(0) as usize;
        let mut actions = Vec::new();
        for (index, line) in self
            .text(uri)
            .lines()
            .enumerate()
            .filter(|(i, _)| (first..=last).contains(i))
        {
            let at = |marker: &str| {
                line.find(marker)
                    .map(|col| range(index, col, index, col + marker.len()))
            };
            let edit_to = |target: &str, range: Value, text: &str| json!({ "changes": { target: [{ "range": range, "newText": text }] } });
            if let Some(found) = at("ERR") {
                actions.push(json!({ "title": "Remove ERR", "kind": "quickfix", "isPreferred": true,
                    "edit": edit_to(uri, found, "") }));
            }
            if let Some(found) = at("WARN") {
                actions.push(json!({ "title": "Fix WARN lazily", "kind": "quickfix",
                    "data": { "uri": uri, "range": found } }));
            }
            if at("CMD").is_some() {
                actions.push(json!({ "title": "Run fake command", "kind": "refactor",
                    "command": { "title": "Run fake command", "command": "fake.run" } }));
            }
            if let Some(found) = at("SECRET") {
                let target = uri
                    .rsplit_once('/')
                    .map_or(String::new(), |(dir, _)| format!("{dir}/.env.rs"));
                actions.push(json!({ "title": "Touch secrets", "kind": "quickfix",
                    "edit": edit_to(&target, found, "x") }));
            }
        }
        Value::Array(actions)
    }

    pub(super) fn resolve_action(&self, action: &Value) -> Value {
        let data = &action["data"];
        let uri = data["uri"].as_str().unwrap_or("");
        let mut resolved = action.clone();
        resolved["edit"] = json!({ "changes": { uri: [{ "range": data["range"], "newText": "ok" }] } });
        resolved
    }

    pub(super) fn formatting(&self, uri: &str) -> Value {
        let edits: Vec<Value> = self
            .text(uri)
            .lines()
            .enumerate()
            .filter(|(_, line)| line.ends_with(' '))
            .map(|(index, line)| {
                let kept = line.trim_end().len();
                json!({ "range": range(index, kept, index, line.len()), "newText": "" })
            })
            .collect();
        Value::Array(edits)
    }

    pub(super) fn workspace_symbols(&self, query: &str) -> Value {
        let found: Vec<Value> = self
            .occurrences(query)
            .into_iter()
            .map(|location| json!({ "name": query, "kind": 12, "location": location }))
            .collect();
        Value::Array(found)
    }
}
