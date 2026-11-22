//! The `lsp` tool's `code_actions` and `format` actions. Both apply their edits through
//! [`LspTool::apply_workspace_edit`], the path `rename` takes.

use super::{LspTool, OpenFile};
use crate::lsp::editorconfig::indent_for;
use crate::lsp::position::{character_offset, locate_symbol};
use rho_harness_core::args::LspArgs;
use serde_json::{Value, json};

/// Actions listed at most; a server can offer hundreds.
const MAX_LISTED: usize = 40;

fn title(action: &Value) -> &str {
    action.get("title").and_then(Value::as_str).unwrap_or("(untitled)")
}

/// A server may answer with bare `Command`s, whose `command` is a string, instead of code actions.
fn command_of(action: &Value) -> Option<&str> {
    match action.get("command")? {
        Value::String(command) => Some(command),
        other => other.get("command").and_then(Value::as_str),
    }
}

fn disabled_reason(action: &Value) -> Option<&str> {
    let disabled = action.get("disabled")?;
    Some(
        disabled
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("no reason given"),
    )
}

fn describe(index: usize, action: &Value) -> String {
    let mut row = match action.get("kind").and_then(Value::as_str) {
        Some(kind) if !kind.is_empty() => format!("{index}. [{kind}] {}", title(action)),
        _ => format!("{index}. {}", title(action)),
    };
    if action.get("isPreferred").and_then(Value::as_bool) == Some(true) {
        row.push_str(" (preferred)");
    }
    if let Some(reason) = disabled_reason(action) {
        row.push_str(&format!(" (disabled: {reason})"));
    } else if action.get("edit").is_none() && command_of(action).is_some() {
        row.push_str(" (runs a server command; rho cannot apply it)");
    }
    row
}

/// The numbered rows of at most [`MAX_LISTED`] actions, and a note on how to apply one.
fn list(actions: &[Value]) -> String {
    let rows: Vec<String> = actions
        .iter()
        .take(MAX_LISTED)
        .enumerate()
        .map(|(index, action)| describe(index + 1, action))
        .collect();
    let hidden = actions.len().saturating_sub(MAX_LISTED);
    let more = if hidden > 0 {
        format!("\n[{hidden} more not shown]")
    } else {
        String::new()
    };
    format!(
        "{}:\n{}{more}\n[apply one with apply=true and query set to its number or part of its title]",
        crate::tools::listing::count_noun(actions.len(), "code action"),
        rows.join("\n")
    )
}

/// The action `query` names: a 1-based number, or a part of exactly one title.
fn pick<'a>(actions: &'a [Value], query: &str) -> Result<&'a Value, String> {
    if let Ok(number) = query.trim().parse::<usize>() {
        return number
            .checked_sub(1)
            .and_then(|index| actions.get(index))
            .ok_or_else(|| format!("there is no action number {number}; the list has {}", actions.len()));
    }
    let wanted = query.to_lowercase();
    let mut matching = actions
        .iter()
        .filter(|action| title(action).to_lowercase().contains(&wanted));
    match (matching.next(), matching.next()) {
        (Some(only), None) => Ok(only),
        (None, _) => Err(format!("no action title contains '{query}'")),
        (Some(_), Some(_)) => Err(format!(
            "'{query}' matches several actions; pass the number from the list"
        )),
    }
}

impl OpenFile {
    /// The range actions are requested for: the symbol's position, else the line, else the file.
    fn action_range(&self, line: Option<usize>, symbol: Option<&str>) -> Result<Value, String> {
        let encoding = self.client.encoding();
        let Some(line) = line else {
            let last = self.text.lines().count().saturating_sub(1);
            let text = self.text.lines().nth(last).unwrap_or("");
            let end = character_offset(text, text.len(), encoding);
            return Ok(json!({ "start": { "line": 0, "character": 0 }, "end": { "line": last, "character": end } }));
        };
        let at = self.position(line, symbol)?;
        if symbol.is_some() {
            return Ok(json!({ "start": at, "end": at }));
        }
        let text = self.text.lines().nth(line - 1).unwrap_or("");
        let end = character_offset(text, text.len(), encoding);
        let start = character_offset(text, locate_symbol(text, None)?, encoding);
        Ok(json!({ "start": { "line": line - 1, "character": start }, "end": { "line": line - 1, "character": end } }))
    }

    /// The server's own diagnostics that touch the requested lines.
    fn context_diagnostics(&self, range: &Value) -> Vec<Value> {
        let first = range["start"]["line"].as_u64().unwrap_or(0);
        let last = range["end"]["line"].as_u64().unwrap_or(first);
        let uri = crate::lsp::client::file_uri(&self.path);
        let published = self.client.state().published_for(&uri);
        published
            .into_iter()
            .filter(|d| {
                let start = d.raw["range"]["start"]["line"].as_u64().unwrap_or(0);
                let end = d.raw["range"]["end"]["line"].as_u64().unwrap_or(start);
                start <= last && end >= first
            })
            .map(|d| d.raw)
            .collect()
    }
}

impl LspTool {
    pub(super) async fn code_actions(&self, args: &LspArgs) -> Result<String, String> {
        let file = self.open(args.path.as_deref()).await?;
        let range = file.action_range(args.line, args.symbol.as_deref())?;
        let context = json!({ "diagnostics": file.context_diagnostics(&range), "triggerKind": 1 });
        let params = json!({ "textDocument": file.text_document(), "range": range, "context": context });
        let response = file.client.request("textDocument/codeAction", params).await?;
        let actions: Vec<Value> = response.as_array().cloned().unwrap_or_default();
        if actions.is_empty() {
            return Ok(file.answer("No code actions available there".into(), true));
        }
        if args.apply != Some(true) {
            return Ok(list(&actions));
        }
        let query = args
            .query
            .as_deref()
            .ok_or("applying a code action needs query: its number or part of its title")?;
        self.apply_action(&file, pick(&actions, query)?).await
    }

    async fn apply_action(&self, file: &OpenFile, action: &Value) -> Result<String, String> {
        if let Some(reason) = disabled_reason(action) {
            return Err(format!("'{}' is disabled: {reason}", title(action)));
        }
        let mut action = action.clone();
        if action.get("edit").is_none() && action.get("data").is_some() {
            action = file.client.request("codeAction/resolve", action).await?;
        }
        let Some(edit) = action.get("edit").filter(|edit| !edit.is_null()) else {
            return Err(match command_of(&action) {
                Some(command) => format!(
                    "'{}' only runs the server command {command}, which rho does not execute; nothing was changed",
                    title(&action)
                ),
                None => format!("'{}' carries no edit; nothing was changed", title(&action)),
            });
        };
        let applied = self.apply_workspace_edit(edit, file, "code action").await?;
        let mut out = format!(
            "Applied '{}' in {}:\n{}{}",
            title(&action),
            crate::tools::listing::count_noun(applied.files.len(), "file"),
            applied.files.join("\n"),
            applied.notes_text()
        );
        if command_of(&action).is_some() {
            out.push_str("\n[the action also asked the server to run a command, which rho does not execute]");
        }
        Ok(out)
    }

    pub(super) async fn format(&self, args: &LspArgs) -> Result<String, String> {
        let file = self.open(args.path.as_deref()).await?;
        if !file.client.supports("documentFormattingProvider") {
            return Err(format!("{} does not support formatting", file.client.name));
        }
        let real = rho_harness_core::workspace::canonicalize_path(&file.path);
        let indent = indent_for(self.manager.workspace(), &real);
        let options = json!({ "tabSize": indent.tab_size, "insertSpaces": indent.insert_spaces });
        let params = json!({ "textDocument": file.text_document(), "options": options });
        let response = file.client.request("textDocument/formatting", params).await?;
        let shown = crate::lsp::render::display_path(&self.base_dir, &file.path);
        if response.as_array().is_none_or(Vec::is_empty) {
            return Ok(format!("{shown} is already formatted"));
        }
        let uri = crate::lsp::client::file_uri(&file.path);
        let edit = json!({ "changes": { uri: response } });
        let applied = self.apply_workspace_edit(&edit, &file, "format").await?;
        let regions = applied.regions.concat();
        if regions.is_empty() {
            return Ok(format!("{shown} is already formatted"));
        }
        let out = format!("Formatted {shown}\n{regions}{}", applied.notes_text());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::describe;
    use serde_json::json;

    #[test]
    fn the_kind_is_bracketed_only_when_the_server_gave_one() {
        let rows = [
            (
                json!({ "title": "Gen", "kind": "refactor.generate" }),
                "1. [refactor.generate] Gen",
            ),
            (json!({ "title": "Gen", "kind": "" }), "1. Gen"),
            (json!({ "title": "Gen" }), "1. Gen"),
            (
                json!({ "title": "Run", "command": "x.run" }),
                "1. Run (runs a server command; rho cannot apply it)",
            ),
            (
                json!({ "title": "Off", "kind": "quickfix", "disabled": { "reason": "no" } }),
                "1. [quickfix] Off (disabled: no)",
            ),
        ];
        for (action, expected) in rows {
            assert_eq!(describe(1, &action), expected);
        }
    }
}
