//! Server slots: one per server and root, from starting through running to exited, plus the policies
//! that cap, prune, and reap them.

use super::{MAX_RUNNING_SERVERS, RETRY_FAILED_AFTER};
use crate::lsp::client::{ClientSpec, LspClient};
use crate::lsp::config::ServerConfig;
use crate::lsp::process::Launcher;
use crate::lsp::support::lock;
use futures::future::{BoxFuture, Shared};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

pub(super) type StartFuture = Shared<BoxFuture<'static, Result<Arc<LspClient>, String>>>;
pub(super) type Key = (String, PathBuf);
pub(super) type Slots = HashMap<Key, Slot>;

/// Failed-start reports remembered so each is shown once; past this the memory starts over.
const MAX_REPORTED_FAILURES: usize = 256;

pub(super) struct Slot {
    pub(super) start: StartFuture,
    pub(super) started: Instant,
    /// Replaces a client that died, so a second death soon after is not restarted at once.
    pub(super) respawn: bool,
}

impl Slot {
    pub(super) fn client(&self) -> Option<&Arc<LspClient>> {
        match self.start.peek() {
            Some(Ok(client)) => Some(client),
            _ => None,
        }
    }

    pub(super) fn running(&self) -> Option<&Arc<LspClient>> {
        self.client().filter(|client| client.is_alive())
    }

    /// Whether a use of this slot's key should reuse, back off from, or replace what it holds.
    pub(super) fn reuse(&self) -> Reuse {
        match self.start.peek() {
            Some(Ok(client)) if client.is_alive() => Reuse::Keep,
            Some(Ok(_)) if self.respawn && self.started.elapsed() < RETRY_FAILED_AFTER => {
                let wait = RETRY_FAILED_AFTER.saturating_sub(self.started.elapsed());
                Reuse::Backoff(format!(
                    "the language server exited again right after a restart; not restarting it for {}s",
                    wait.as_secs() + 1
                ))
            }
            Some(Ok(_)) => Reuse::Replace { respawn: true },
            Some(Err(_)) if self.started.elapsed() < RETRY_FAILED_AFTER => Reuse::Keep,
            Some(Err(_)) => Reuse::Replace { respawn: false },
            None => Reuse::Keep,
        }
    }
}

pub(super) enum Reuse {
    Keep,
    Backoff(String),
    Replace { respawn: bool },
}

/// Whether `key` has not been reported yet; remembers it.
pub(super) fn first_report(reported: &mut HashSet<Key>, key: Key) -> bool {
    if reported.len() >= MAX_REPORTED_FAILURES && !reported.contains(&key) {
        reported.clear();
    }
    reported.insert(key)
}

/// Forgets servers that exited long enough ago that restarting one is no longer held back.
pub(super) fn prune_exited(slots: &mut Slots) {
    slots.retain(|_, slot| {
        slot.running().is_some() || slot.client().is_none() || slot.started.elapsed() < RETRY_FAILED_AFTER
    });
}

/// Stops the server that has been idle longest when the cap on servers is reached. Servers still
/// starting count toward the cap, aged by when they started.
pub(super) fn stop_least_recently_used(slots: &mut Slots) {
    let live = slots
        .values()
        .filter(|slot| slot.start.peek().is_none() || slot.running().is_some())
        .count();
    if live < MAX_RUNNING_SERVERS {
        return;
    }
    let idlest = slots
        .iter()
        .filter_map(|(key, slot)| {
            let idle = match slot.start.peek() {
                None => slot.started.elapsed(),
                Some(_) => slot.running()?.idle_for(),
            };
            Some((key, idle))
        })
        .max_by_key(|(_, idle)| *idle)
        .map(|(key, _)| key.clone());
    let stopped = idlest
        .and_then(|key| slots.remove(&key))
        .and_then(|slot| slot.running().cloned());
    if let Some(client) = stopped {
        tokio::spawn(async move { client.shutdown().await });
    }
}

/// Shuts down the servers unused for `idle`; zero keeps them forever.
pub(super) async fn reap_idle(slots: &Mutex<Slots>, idle: Duration) {
    if idle.is_zero() {
        return;
    }
    let stale: Vec<Arc<LspClient>> = {
        let mut slots = lock(slots);
        let keys: Vec<Key> = slots
            .iter()
            .filter(|(_, slot)| slot.running().is_some_and(|client| client.idle_for() >= idle))
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter()
            .filter_map(|key| slots.remove(key))
            .filter_map(|slot| slot.running().cloned())
            .collect()
    };
    futures::future::join_all(stale.iter().map(|client| client.shutdown())).await;
}

pub(super) async fn start_client(
    launcher: Arc<dyn Launcher>,
    server: ServerConfig,
    command: PathBuf,
    root: PathBuf,
) -> Result<Arc<LspClient>, String> {
    let launched = launcher.launch(&server, &command, &root)?;
    let stderr = launched.stderr.clone();
    let spec = ClientSpec {
        name: &server.name,
        root: &root,
        initialization_options: server.initialization_options.clone(),
        settings: server.settings.clone(),
    };
    match LspClient::start(spec, launched.reader, launched.writer, launched.process).await {
        Ok(client) => Ok(Arc::new(client)),
        Err(error) => {
            let deadline = Instant::now() + Duration::from_millis(200);
            while stderr.last_lines(1).is_empty() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let output = stderr.last_lines(3);
            let output = if output.is_empty() {
                String::new()
            } else {
                format!("; stderr: {output}")
            };
            Err(format!("{} failed to initialize: {error}{output}", server.name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_failure_reports_are_bounded() {
        let mut reported = HashSet::new();
        let key = |index: usize| (format!("server{index}"), PathBuf::from("/ws"));
        assert!(first_report(&mut reported, key(0)));
        assert!(!first_report(&mut reported, key(0)));
        for index in 1..MAX_REPORTED_FAILURES * 3 {
            first_report(&mut reported, key(index));
        }
        assert!(reported.len() <= MAX_REPORTED_FAILURES, "{}", reported.len());
    }
}
