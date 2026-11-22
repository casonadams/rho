use super::LspTool;
use crate::lsp::{LspClient, render};
use crate::permission::path::{is_credential_file, is_protected_path};
use crate::tools::atomic::atomic_write;
use rho_harness_core::workspace::canonicalize_path;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One file's rename: the contents to restore and the contents to write.
struct Planned {
    path: PathBuf,
    before: String,
    next: String,
    count: usize,
}

/// The files an edit touches, resolved through symlinks. The server names them, so they are
/// checked here against the workspace and the same protected paths `write` and `edit` ask about.
pub(super) fn confine(
    by_file: BTreeMap<PathBuf, Vec<Value>>,
    workspace: &Path,
    what: &str,
) -> Result<BTreeMap<PathBuf, Vec<Value>>, String> {
    let workspace = canonicalize_path(workspace);
    let mut confined: BTreeMap<PathBuf, Vec<Value>> = BTreeMap::new();
    for (path, edits) in by_file {
        let real = canonicalize_path(&path);
        if !real.starts_with(&workspace) {
            return Err(format!(
                "{what} would change {}, outside the workspace; nothing was written",
                path.display()
            ));
        }
        if is_protected_path(&real.to_string_lossy(), Some(&workspace)) || is_credential_file(&real) {
            return Err(format!(
                "{what} would change {}, a protected path (.env*, .git, .rho, or a config file); nothing was written",
                real.display()
            ));
        }
        confined.entry(real).or_default().extend(edits);
    }
    Ok(confined)
}

/// Writes every file, or restores the ones already written when a write fails.
async fn write_all(planned: &[Planned]) -> Result<(), String> {
    for (index, file) in planned.iter().enumerate() {
        let Err(error) = atomic_write(&file.path, file.next.as_bytes()).await else {
            continue;
        };
        let mut failed = Vec::new();
        for written in &planned[..index] {
            if let Err(restore) = atomic_write(&written.path, written.before.as_bytes()).await {
                failed.push(format!("{}: {restore}", written.path.display()));
            }
        }
        let outcome = if failed.is_empty() {
            format!(
                "rolled back {} already written",
                crate::tools::listing::count_noun(index, "file")
            )
        } else {
            format!("could not restore: {}", failed.join("; "))
        };
        return Err(format!("writing {}: {error}; {outcome}", file.path.display()));
    }
    Ok(())
}

/// What applying a `WorkspaceEdit` wrote: one line per file, and trouble telling the server.
pub(super) struct Applied {
    pub files: Vec<String>,
    pub notes: Vec<String>,
    /// How each file changed, as `edit` shows it, in the order of `files`.
    pub regions: Vec<String>,
}

impl LspTool {
    pub(super) async fn rename(&self, args: &rho_harness_core::args::LspArgs) -> Result<String, String> {
        let new_name = args.new_name.clone().ok_or("rename needs new_name")?;
        let (file, response) = self
            .position_request(args, "textDocument/rename", json!({ "newName": new_name }))
            .await?;
        if response.is_null() {
            return Err(format!("{} found nothing to rename there", file.client.name));
        }
        let applied = self.apply_workspace_edit(&response, &file.client, "rename").await?;
        let mut out = format!(
            "Renamed to {new_name} in {}:\n{}",
            crate::tools::listing::count_noun(applied.files.len(), "file"),
            applied.files.join("\n")
        );
        for note in applied.notes {
            out.push_str(&format!("\n[{note}]"));
        }
        Ok(out)
    }

    /// Writes every file a `WorkspaceEdit` changes, all or none, records each in the edit history,
    /// and tells `client` and the other servers. `what` names the action in refusals.
    pub(super) async fn apply_workspace_edit(
        &self,
        edit: &Value,
        client: &LspClient,
        what: &str,
    ) -> Result<Applied, String> {
        let by_file = confine(render::workspace_edits(edit)?, self.manager.workspace(), what)?;
        let mut planned = Vec::new();
        for (path, edits) in by_file {
            let before = tokio::fs::read_to_string(&path)
                .await
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            let next = render::apply_text_edits(&before, &edits, client.encoding())?;
            planned.push(Planned {
                path,
                before,
                next,
                count: edits.len(),
            });
        }
        write_all(&planned).await?;
        let mut applied = Applied {
            files: Vec::new(),
            notes: Vec::new(),
            regions: Vec::new(),
        };
        for file_plan in &planned {
            applied.regions.push(crate::tools::edit::preview::changed_regions(
                &file_plan.before,
                &file_plan.next,
            ));
            if let Some(history) = &self.history {
                history.record(
                    &file_plan.path,
                    Some(file_plan.before.clone().into_bytes()),
                    file_plan.next.as_bytes(),
                );
            }
            let language = self.language_id(client, &file_plan.path);
            let synced = client.sync(&file_plan.path, &file_plan.next, &language).await;
            if let Err(error) = synced.and(client.did_save(&file_plan.path, &file_plan.next).await) {
                applied.notes.push(format!(
                    "{} was written but the server was not told: {error}",
                    file_plan.path.display()
                ));
            }
            applied.files.push(format!(
                "{} ({})",
                render::display_path(&self.base_dir, &file_plan.path),
                file_plan.count
            ));
        }
        let paths: Vec<PathBuf> = planned.iter().map(|file_plan| file_plan.path.clone()).collect();
        self.manager.files_changed(&paths);
        Ok(applied)
    }
}
