use super::{LspTool, OpenFile, read_text};
use crate::lsp::{LspClient, render};
use crate::permission::path::{is_credential_file, is_protected_path};
use crate::tools::atomic::atomic_write;
use rho_harness_core::workspace::canonicalize_path;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One file's rename: the contents to restore and the contents to write.
pub(super) struct Planned {
    pub path: PathBuf,
    pub before: String,
    pub next: String,
    pub count: usize,
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

/// A failed all-or-nothing write: what to tell the model, and the indices of the planned files
/// that still hold their new contents because they could not be restored.
struct WriteFailure {
    message: String,
    left_changed: Vec<usize>,
}

/// Writes every file, or restores the ones already written when a write fails.
async fn write_all(
    planned: &[Planned],
    write: impl AsyncFn(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), WriteFailure> {
    for (index, file) in planned.iter().enumerate() {
        let Err(error) = write(&file.path, file.next.as_bytes()).await else {
            continue;
        };
        let mut left_changed = Vec::new();
        let mut failed = Vec::new();
        for (written_index, written) in planned[..index].iter().enumerate() {
            if let Err(restore) = write(&written.path, written.before.as_bytes()).await {
                left_changed.push(written_index);
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
        return Err(WriteFailure {
            message: format!("writing {}: {error}; {outcome}", file.path.display()),
            left_changed,
        });
    }
    Ok(())
}

/// Refuses the edit when a file is no longer what the plan was made from.
async fn ensure_unchanged(planned: &[Planned], what: &str) -> Result<(), String> {
    for file in planned {
        if tokio::fs::read_to_string(&file.path).await.ok().as_deref() != Some(file.before.as_str()) {
            return Err(stale(&file.path, what));
        }
    }
    Ok(())
}

fn stale(path: &Path, what: &str) -> String {
    format!(
        "{} changed on disk after the language server read it, so its {what} edits no longer fit; nothing was written. Retry the {what}",
        path.display()
    )
}

/// What applying a `WorkspaceEdit` wrote: one line per file, and trouble telling the server.
pub(super) struct Applied {
    pub files: Vec<String>,
    pub notes: Vec<String>,
    /// How each file changed, as `edit` shows it, in the order of `files`.
    pub regions: Vec<String>,
}

impl Applied {
    /// One bracketed line per note, each after a newline.
    pub(super) fn notes_text(&self) -> String {
        self.notes.iter().map(|note| format!("\n[{note}]")).collect()
    }
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
        let applied = self.apply_workspace_edit(&response, &file, "rename").await?;
        if applied.files.is_empty() {
            return Ok(format!("Renaming to {new_name} changed nothing; no file was written"));
        }
        let out = format!(
            "Renamed to {new_name} in {}:\n{}{}",
            crate::tools::listing::count_noun(applied.files.len(), "file"),
            applied.files.join("\n"),
            applied.notes_text()
        );
        Ok(out)
    }

    /// Writes every file a `WorkspaceEdit` changes, all or none, records each in the edit history,
    /// and tells `file`'s server and the other servers. `what` names the action in refusals.
    /// The edit was computed against the text `file` was synced with, so it is refused if that
    /// file changed on disk since, or if any file changes between planning and writing.
    pub(super) async fn apply_workspace_edit(
        &self,
        edit: &Value,
        file: &OpenFile,
        what: &str,
    ) -> Result<Applied, String> {
        let by_file = confine(render::workspace_edits(edit)?, self.manager.workspace(), what)?;
        let opened = canonicalize_path(&file.path);
        let mut planned = Vec::new();
        for (path, edits) in by_file {
            let before = read_text(&path).await?;
            if path == opened && before != file.text {
                return Err(stale(&path, what));
            }
            let next = render::apply_text_edits(&before, &edits, file.client.encoding())?;
            if next != before {
                planned.push(Planned {
                    path,
                    before,
                    next,
                    count: edits.len(),
                });
            }
        }
        ensure_unchanged(&planned, what).await?;
        self.commit(planned, &file.client, atomic_write).await
    }

    /// Tells the servers about, and records in the history, the files that now hold new contents.
    async fn announce(&self, changed: &[&Planned], client: &LspClient) -> Applied {
        let mut applied = Applied {
            files: Vec::new(),
            notes: Vec::new(),
            regions: Vec::new(),
        };
        for file_plan in changed {
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
                render::display_path(&self.base_dir.get(), &file_plan.path),
                file_plan.count
            ));
        }
        if !changed.is_empty() {
            let paths: Vec<PathBuf> = changed.iter().map(|file_plan| file_plan.path.clone()).collect();
            self.manager.files_changed(&paths);
        }
        applied
    }

    pub(super) async fn commit(
        &self,
        planned: Vec<Planned>,
        client: &LspClient,
        write: impl AsyncFn(&Path, &[u8]) -> std::io::Result<()>,
    ) -> Result<Applied, String> {
        if let Err(failure) = write_all(&planned, write).await {
            let left: Vec<&Planned> = failure.left_changed.iter().map(|&index| &planned[index]).collect();
            self.announce(&left, client).await;
            return Err(failure.message);
        }
        let written: Vec<&Planned> = planned.iter().collect();
        Ok(self.announce(&written, client).await)
    }
}
