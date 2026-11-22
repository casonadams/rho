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
pub use manager::{Launched, Launcher, LspManager};

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// What every server for one file reported.
#[derive(Default)]
struct Report {
    found: Vec<Diagnostic>,
    answered: bool,
    complete: bool,
    other_errors: usize,
    notes: Vec<String>,
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
    let mut report = Report {
        complete: true,
        ..Report::default()
    };
    for client in clients {
        match client {
            Ok(client) => collect(&client, manager, path, text, deadline, &mut report).await,
            Err(error) => {
                report.complete = false;
                report.notes.push(error);
            }
        }
    }
    report.found.retain(|d| d.severity <= 2);
    render::sort_diagnostics(&mut report.found);
    Some(render_report(&report))
}

async fn collect(
    client: &Arc<LspClient>,
    manager: &LspManager,
    path: &Path,
    text: &str,
    deadline: Instant,
    report: &mut Report,
) {
    let language = manager
        .servers()
        .iter()
        .find(|s| s.name == client.name)
        .map_or_else(String::new, |s| s.language_id(path));
    let synced = match client.sync(path, text, &language).await {
        Ok(synced) => synced,
        Err(error) => return report.notes.push(format!("{}: {error}", client.name)),
    };
    if let Err(error) = client.did_save(path, text).await {
        return report.notes.push(format!("{}: {error}", client.name));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let items = match client.diagnostics(path, synced, remaining).await {
        DiagnosticsOutcome::Ready(items) => items,
        DiagnosticsOutcome::Partial(items, busy) => {
            report.complete = false;
            report.notes.push(format!(
                "{} is still busy ({busy}); more diagnostics may follow",
                client.name
            ));
            items
        }
        DiagnosticsOutcome::Pending(busy) => {
            report.complete = false;
            let note = match busy {
                _ if !client.is_alive() => format!("{} exited", client.name),
                Some(busy) => format!("{} has not reported diagnostics yet ({busy})", client.name),
                None => format!("{} has not reported diagnostics yet", client.name),
            };
            return report.notes.push(note);
        }
    };
    let uri = client::file_uri(path);
    report.other_errors += client
        .state()
        .all_published()
        .iter()
        .filter(|(other, _)| *other != uri)
        .map(|(_, items)| items.iter().filter(|d| d.severity == 1).count())
        .sum::<usize>();
    report.answered = true;
    report.found.extend(items);
}

fn render_report(report: &Report) -> String {
    let label = if report.complete { "LSP" } else { "LSP so far" };
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
