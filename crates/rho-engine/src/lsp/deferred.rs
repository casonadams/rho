//! Diagnostics that miss an edit's short wait: a background wait keeps going and, when the server
//! reports something new, delivers it to the agent's mailbox like a finished background job.

use super::{Diagnostic, Initiator, LspManager, Pass, Report, check, render_report};
use crate::mailbox::{Mailbox, NoticeTarget, framed_notice};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// How long after an edit the servers are still waited for.
pub const FOLLOW_UP_WAIT: Duration = Duration::from_secs(12);
/// Waits running at once across all files; further edits only report what the short wait found.
pub const MAX_FOLLOW_UPS: usize = 8;

/// A running wait's place among the [`MAX_FOLLOW_UPS`].
struct Claim(Arc<LspManager>);

impl Claim {
    fn take(manager: &Arc<LspManager>) -> Option<Self> {
        let running = manager.epochs.follow_ups.fetch_add(1, Ordering::SeqCst);
        let claim = Self(Arc::clone(manager));
        (running < MAX_FOLLOW_UPS).then_some(claim)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.0.epochs.follow_ups.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Keeps waiting for `path`'s diagnostics after `shown` (what the edit reported) was given to the
/// model, and mails what arrives unless it is the same, or the file changed again (`stamp` is its
/// change count when `text` was written).
pub(super) fn follow_up(
    manager: &Arc<LspManager>,
    inbox: &Mailbox,
    path: &Path,
    text: &str,
    shown: &Report,
    stamp: u64,
) {
    let Some(claim) = Claim::take(manager) else {
        return;
    };
    let wait = Wait {
        manager: Arc::clone(manager),
        owner: inbox.notices(),
        path: path.to_path_buf(),
        text: text.to_string(),
        shown: shown.answered.then(|| shown.found.clone()),
        stamp,
    };
    tokio::spawn(async move {
        wait.run().await;
        drop(claim);
    });
}

struct Wait {
    manager: Arc<LspManager>,
    owner: NoticeTarget,
    path: PathBuf,
    text: String,
    shown: Option<Vec<Diagnostic>>,
    stamp: u64,
}

impl Wait {
    async fn run(self) {
        let Some(report) = check(
            &self.manager,
            &self.path,
            &self.text,
            FOLLOW_UP_WAIT,
            Pass::FollowUp,
            Initiator::Automatic,
        )
        .await
        else {
            return;
        };
        let stale = self.manager.change_count(&self.path) != self.stamp;
        if stale || !report.answered || self.shown.as_ref() == Some(&report.found) {
            return;
        }
        let heading = format!(
            "Language server diagnostics for {} arrived after the edit",
            self.manager.relative(&self.path)
        );
        let notice = framed_notice(&heading, Some(&render_report(&report)));
        if let Err(error) = self.owner.send(notice) {
            tracing::debug!(
                "late diagnostics for {} were not delivered: {error}",
                self.path.display()
            );
        }
    }
}
