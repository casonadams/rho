//! Interactive programs (editors, REPLs, TUIs) driven through pseudo-terminals.

mod keys;
mod screen;
mod session;
#[cfg(test)]
mod tests;

use crate::tools::engine_tool::TypedTool;
use crate::tools::types::ToolResult;
use keys::input_bytes;
use rho_harness_core::args::{TerminalAction, TerminalArgs};
use rho_harness_core::error::AppError;
use session::{TermSize, TerminalSession, Wait};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

const MAX_SESSIONS: usize = 8;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 40;
const DEFAULT_WAIT_MS: u64 = 2000;
const MAX_WAIT_MS: u64 = 60_000;

#[derive(Default)]
struct Sessions {
    last_id: u32,
    live: BTreeMap<u32, Arc<TerminalSession>>,
}

/// The `terminal` tool. Its sessions live as long as the tool and are killed when it is dropped.
pub struct TerminalTool {
    base_dir: PathBuf,
    sessions: Mutex<Sessions>,
}

impl TerminalTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            sessions: Mutex::default(),
        }
    }

    async fn dispatch(&self, args: TerminalArgs) -> Result<String, String> {
        match args.action {
            TerminalAction::Start => self.start(args).await,
            TerminalAction::Send => self.send(args).await,
            TerminalAction::Screen => Ok(self.session(args.id)?.render()),
            TerminalAction::Resize => self.resize(&args),
            TerminalAction::Close => self.close(args.id).await,
            TerminalAction::List => Ok(self.list()),
        }
    }

    async fn start(&self, args: TerminalArgs) -> Result<String, String> {
        let command = args
            .command
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or("start needs a command")?;
        let size = TermSize::new(args.cols.unwrap_or(DEFAULT_COLS), args.rows.unwrap_or(DEFAULT_ROWS))?;
        let wait = wait_for(&args);
        let session = {
            let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            if sessions.live.len() >= MAX_SESSIONS {
                return Err(format!(
                    "{MAX_SESSIONS} terminal sessions are open; close one before starting another"
                ));
            }
            let id = sessions.last_id + 1;
            let session = Arc::new(TerminalSession::spawn(id, command, &self.base_dir, size)?);
            sessions.last_id = id;
            sessions.live.insert(id, session.clone());
            session
        };
        session.settle(session.subscribe(), &wait).await;
        Ok(rendered_after_wait(&session, &wait))
    }

    async fn send(&self, args: TerminalArgs) -> Result<String, String> {
        let session = self.session(args.id)?;
        if !session.is_running() {
            return Err(format!(
                "{}\nThe program has exited; close the session.",
                session.summary()
            ));
        }
        let keys = args.keys.as_deref().unwrap_or_default();
        let bytes = input_bytes(
            args.text.as_deref().unwrap_or_default(),
            keys,
            session.application_cursor(),
        )?;
        if bytes.is_empty() {
            return Err("send needs text or keys".to_string());
        }
        let wait = wait_for(&args);
        let output = session.subscribe();
        session.write(bytes).await?;
        session.settle(output, &wait).await;
        Ok(rendered_after_wait(&session, &wait))
    }

    fn resize(&self, args: &TerminalArgs) -> Result<String, String> {
        let session = self.session(args.id)?;
        let (Some(cols), Some(rows)) = (args.cols, args.rows) else {
            return Err("resize needs cols and rows".to_string());
        };
        session.resize(TermSize::new(cols, rows)?)?;
        Ok(session.render())
    }

    async fn close(&self, id: Option<u32>) -> Result<String, String> {
        let session = self.session(id)?;
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .live
            .remove(&session.id);
        let summary = session.summary();
        let ended = tokio::task::spawn_blocking(move || session.terminate())
            .await
            .map_err(|e| format!("closing the terminal failed: {e}"))?;
        Ok(format!("{summary}\nClosed: {ended}."))
    }

    fn list(&self) -> String {
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if sessions.live.is_empty() {
            return "No terminal sessions.".to_string();
        }
        let lines: Vec<String> = sessions.live.values().map(|s| s.summary()).collect();
        lines.join("\n")
    }

    fn session(&self, id: Option<u32>) -> Result<Arc<TerminalSession>, String> {
        let id = id.ok_or("this action needs the session id")?;
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        sessions
            .live
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no terminal session {id}; use action 'list' to see open sessions"))
    }
}

fn wait_for(args: &TerminalArgs) -> Wait {
    Wait {
        limit: Duration::from_millis(args.wait_ms.unwrap_or(DEFAULT_WAIT_MS).min(MAX_WAIT_MS)),
        until: args.wait_for.clone().filter(|text| !text.is_empty()),
    }
}

fn rendered_after_wait(session: &TerminalSession, wait: &Wait) -> String {
    let screen = session.render();
    match &wait.until {
        Some(text) if !session.shows(text) => format!(
            "{screen}\n[\"{text}\" did not appear within {} ms]",
            wait.limit.as_millis()
        ),
        _ => screen,
    }
}

#[async_trait::async_trait]
impl TypedTool for TerminalTool {
    type Args = TerminalArgs;
    const NAME: &'static str = "terminal";

    async fn run(&self, args: TerminalArgs) -> Result<ToolResult, AppError> {
        Ok(match self.dispatch(args).await {
            Ok(text) => ToolResult::success(text),
            Err(error) => ToolResult::error(error),
        })
    }
}
