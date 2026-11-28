use crate::tools::engine_tool::TypedTool;
use crate::tools::types::ToolResult;
use rho_harness_core::args::{MemoryAction, MemoryArgs, MemoryScope};
use rho_harness_core::error::AppError;
use rho_harness_core::memory::{MemoryError, MemoryStore, numbered};

const NEXT_SESSION: &str = "Memory loads at session start, so this takes effect in the next session.";

/// Lets the model keep short notes across sessions in the user's memory directory.
pub(crate) struct MemoryTool {
    store: MemoryStore,
}

impl MemoryTool {
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl TypedTool for MemoryTool {
    type Args = MemoryArgs;
    const NAME: &'static str = "memory";

    async fn run(&self, args: MemoryArgs) -> Result<ToolResult, AppError> {
        let store = self.store.clone();
        let outcome = tokio::task::spawn_blocking(move || run_action(&store, args))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("memory task failed: {e}")))?;
        Ok(match outcome {
            Ok(text) => ToolResult::success(text),
            Err(error) => ToolResult::error(error),
        })
    }
}

fn run_action(store: &MemoryStore, args: MemoryArgs) -> Result<String, String> {
    let scope = args.scope.unwrap_or_default();
    match args.action {
        MemoryAction::View => view(store, args.scope).map_err(|e| e.to_string()),
        MemoryAction::Add => {
            let text = args.text.ok_or("add needs `text`")?;
            let number = store.add(scope, &text).map_err(|e| e.to_string())?;
            Ok(format!("Saved {} note {number}. {NEXT_SESSION}", scope.as_str()))
        }
        MemoryAction::Remove => {
            let index = args.index.ok_or("remove needs `index` (a note number from view)")?;
            let removed = store.remove(scope, index).map_err(|e| e.to_string())?;
            Ok(format!(
                "Removed {} note {index}: {removed}\nLater notes moved up one number. {NEXT_SESSION}",
                scope.as_str()
            ))
        }
    }
}

fn view(store: &MemoryStore, scope: Option<MemoryScope>) -> Result<String, MemoryError> {
    let scopes = match scope {
        Some(scope) => vec![scope],
        None => vec![MemoryScope::Project, MemoryScope::Global],
    };
    let mut out = String::new();
    for scope in scopes {
        let notes = store.notes(scope)?;
        let listed = if notes.is_empty() {
            "(none)\n".to_string()
        } else {
            numbered(&notes)
        };
        let label = match scope {
            MemoryScope::Project => "Project",
            MemoryScope::Global => "Global",
        };
        out.push_str(&format!("{label} notes ({}):\n{listed}", store.path(scope).display()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::engine_tool::EngineTool;
    use serde_json::json;

    async fn call(tool: &MemoryTool, args: serde_json::Value) -> ToolResult {
        tool.execute(args).await.unwrap()
    }

    #[tokio::test]
    async fn the_model_adds_views_and_removes_notes_that_apply_next_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().join("memory"), dir.path());
        let tool = MemoryTool::new(store.clone());

        let added = call(&tool, json!({ "action": "add", "text": "fidelity tests are slow" })).await;
        call(
            &tool,
            json!({ "action": "add", "scope": "global", "text": "prefer rg" }),
        )
        .await;
        let duplicate = call(&tool, json!({ "action": "add", "text": "Fidelity tests are slow" })).await;
        let view = call(&tool, json!({ "action": "view" })).await.content;
        let removed = call(&tool, json!({ "action": "remove", "scope": "global", "index": 1 })).await;
        let global = call(&tool, json!({ "action": "view", "scope": "global" }))
            .await
            .content;

        assert!(
            !added.is_error && added.content.contains("next session"),
            "{}",
            added.content
        );
        assert!(
            duplicate.is_error && duplicate.content.contains("already"),
            "{}",
            duplicate.content
        );
        assert!(
            view.contains("Project notes (") && view.contains("1. fidelity tests are slow"),
            "{view}"
        );
        assert!(
            view.contains("Global notes (") && view.contains("1. prefer rg"),
            "{view}"
        );
        assert!(
            removed.content.starts_with("Removed global note 1: prefer rg"),
            "{}",
            removed.content
        );
        assert!(!global.contains("Project") && global.contains("(none)"), "{global}");
        assert_eq!(store.notes(MemoryScope::Project).unwrap(), ["fidelity tests are slow"]);
    }

    #[tokio::test]
    async fn actions_missing_their_argument_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let tool = MemoryTool::new(MemoryStore::new(dir.path().join("memory"), dir.path()));
        let add = call(&tool, json!({ "action": "add" })).await;
        let remove = call(&tool, json!({ "action": "remove" })).await;
        assert!(add.is_error && add.content.contains("needs `text`"), "{}", add.content);
        assert!(
            remove.is_error && remove.content.contains("needs `index`"),
            "{}",
            remove.content
        );
    }

    #[tokio::test]
    async fn removing_a_missing_note_number_is_an_error_that_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().join("memory"), dir.path());
        let tool = MemoryTool::new(store.clone());
        call(&tool, json!({ "action": "add", "text": "only note" })).await;
        for index in [0, 2] {
            let result = call(&tool, json!({ "action": "remove", "index": index })).await;
            assert!(
                result.is_error && result.content.contains("no note"),
                "{}",
                result.content
            );
        }
        assert_eq!(store.notes(MemoryScope::Project).unwrap(), ["only note"]);
    }

    #[tokio::test]
    async fn a_note_that_looks_like_a_credential_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().join("memory"), dir.path());
        let tool = MemoryTool::new(store.clone());
        let result = call(
            &tool,
            json!({ "action": "add", "text": "api key sk-abcdefghijklmnopqrstuvwx" }),
        )
        .await;
        assert!(
            result.is_error && result.content.contains("credential"),
            "{}",
            result.content
        );
        assert!(store.notes(MemoryScope::Project).unwrap().is_empty());
    }
}
