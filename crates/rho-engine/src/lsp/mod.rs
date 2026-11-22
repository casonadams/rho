//! Language Server Protocol client: server configuration, process lifecycle, document sync, and
//! the queries behind the `lsp` tool and post-edit diagnostics.

pub mod client;
pub mod config;
pub mod manager;
pub mod position;
pub mod process;
pub mod render;
pub mod rpc;
pub mod state;
#[cfg(test)]
pub(crate) mod test_server;

pub use client::{Diagnostic, DiagnosticsOutcome, LspClient};
pub use manager::{Launched, Launcher, LspManager, shutdown_all_blocking, shutdown_all_managers};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub const EDIT_DIAGNOSTICS_WAIT: Duration = Duration::from_secs(3);

/// Appends the file's diagnostics to a successful edit or write result.
pub async fn append_diagnostics(
    mut result: crate::tools::types::ToolResult,
    manager: Option<&LspManager>,
    path: &Path,
    text: &str,
) -> crate::tools::types::ToolResult {
    let Some(manager) = manager.filter(|_| !result.is_error) else {
        return result;
    };
    if let Some(report) = diagnostics_for(manager, path, text, EDIT_DIAGNOSTICS_WAIT).await {
        result.content.push_str("\n\n");
        result.content.push_str(&report);
    }
    result
}

/// What the servers for one file reported.
#[derive(Default)]
struct Report {
    found: Vec<Diagnostic>,
    answered: bool,
    incomplete: bool,
    other_errors: usize,
    notes: Vec<String>,
}

impl Report {
    fn noted(note: String) -> Self {
        Self {
            notes: vec![note],
            ..Self::default()
        }
    }

    fn merge(&mut self, other: Report) {
        self.found.extend(other.found);
        self.answered |= other.answered;
        self.incomplete |= other.incomplete;
        self.other_errors += other.other_errors;
        self.notes.extend(other.notes);
    }
}

/// The diagnostics every server for `path` reports once it holds `text`, waiting at most `wait`
/// in total. `None` when no server handles the file, or it lies outside the workspace (a server
/// would have no project for it and report it clean).
pub async fn diagnostics_for(manager: &LspManager, path: &Path, text: &str, wait: Duration) -> Option<String> {
    if !manager.in_workspace(path) {
        return None;
    }
    let deadline = Instant::now() + wait;
    let clients = manager.clients_within(path, wait).await;
    if clients.is_empty() {
        return None;
    }
    let reports = futures::future::join_all(clients.into_iter().map(|client| async {
        match client {
            Ok(client) => collect(&client, manager, path, text, deadline).await,
            Err(error) => Report {
                incomplete: true,
                ..Report::noted(error)
            },
        }
    }))
    .await;
    let mut report = Report::default();
    for part in reports {
        report.merge(part);
    }
    report.found.retain(|d| d.severity <= 2);
    render::sort_diagnostics(&mut report.found);
    Some(render_report(&report))
}

async fn collect(client: &Arc<LspClient>, manager: &LspManager, path: &Path, text: &str, deadline: Instant) -> Report {
    let language = manager
        .servers()
        .iter()
        .find(|s| s.name == client.name)
        .map_or_else(String::new, |s| s.language_id(path));
    let synced = match client.sync(path, text, &language).await {
        Ok(synced) => synced,
        Err(error) => return Report::noted(format!("{}: {error}", client.name)),
    };
    if let Err(error) = client.did_save(path, text).await {
        return Report::noted(format!("{}: {error}", client.name));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let shown = manager.relative(path);
    let follow_up = format!("run lsp diagnostics path={shown} in a moment");
    let (items, incomplete, note) = match client.diagnostics(path, synced, remaining).await {
        DiagnosticsOutcome::Ready(items) => (items, false, None),
        DiagnosticsOutcome::Partial(items, busy) => {
            let note = format!("{} is still busy ({busy}); {follow_up} for the rest", client.name);
            (items, true, Some(note))
        }
        DiagnosticsOutcome::Pending(busy) => {
            let note = match busy {
                _ if !client.is_alive() => format!("{} exited", client.name),
                Some(busy) => format!("{} has not reported diagnostics yet ({busy}); {follow_up}", client.name),
                None => format!("{} has not reported diagnostics yet; {follow_up}", client.name),
            };
            return Report {
                incomplete: true,
                ..Report::noted(note)
            };
        }
    };
    Report {
        other_errors: errors_in_other_files(client, &client::file_uri(path)),
        found: items,
        answered: true,
        incomplete,
        notes: note.into_iter().collect(),
    }
}

/// Errors the server holds for files other than `uri` that still exist on disk.
fn errors_in_other_files(client: &LspClient, uri: &str) -> usize {
    let mut count = 0;
    for (other, items) in client
        .state()
        .all_published()
        .into_iter()
        .filter(|(other, _)| other != uri)
    {
        let errors = items.iter().filter(|d| d.severity == 1).count();
        let exists = match client::uri_path(&other) {
            Some(path) if errors > 0 => path.exists(),
            _ => false,
        };
        if exists {
            count += errors;
        }
    }
    count
}

fn render_report(report: &Report) -> String {
    let label = if report.incomplete { "LSP so far" } else { "LSP" };
    let mut out = if !report.answered {
        "LSP: no diagnostics yet".to_string()
    } else if report.found.is_empty() {
        format!("{label}: no errors or warnings")
    } else {
        format!(
            "{label} ({}):\n{}",
            render::count_summary(&report.found),
            render::diagnostic_rows(&report.found)
        )
    };
    if report.other_errors > 0 {
        out.push_str(&format!(
            "\n[{} in other files; lsp diagnostics lists them]",
            crate::tools::listing::count_noun(report.other_errors, "error")
        ));
    }
    if !report.notes.is_empty() {
        out.push_str(&format!("\n[{}]", report.notes.join("; ")));
    }
    out
}

#[cfg(test)]
mod tests;
