//! Interactive programs (editors, REPLs, TUIs) driven through pseudo-terminals.

mod keys;
mod screen;
mod session;
#[cfg(test)]
mod tests;

use crate::tools::engine_tool::TypedTool;
use crate::tools::types::ToolResult;
use keys::input_segments;
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
const MAX_SCROLLBACK: usize = 200;
const START_PATIENCE: Duration = Duration::from_millis(600);

#[derive(Default)]
struct Sessions {
    last_id: u32,
    live: BTreeMap<u32, Arc<TerminalSession>>,
}

/// Removes a just-started session again unless the call that started it got to return its id.
struct Registration<'a> {
    sessions: &'a Mutex<Sessions>,
    id: u32,
    returned: bool,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        if !self.returned {
            lock_sessions(self.sessions).live.remove(&self.id);
        }
    }
}

fn lock_sessions(sessions: &Mutex<Sessions>) -> std::sync::MutexGuard<'_, Sessions> {
    sessions.lock().unwrap_or_else(PoisonError::into_inner)
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
            TerminalAction::Screen => Ok(self.session(args.id)?.render(scrollback_lines(&args))),
            TerminalAction::Resize => self.resize(&args).await,
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
        let wait = wait_for(&args).patient(START_PATIENCE);
        let session = {
            let mut sessions = lock_sessions(&self.sessions);
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
        let mut registration = Registration {
            sessions: &self.sessions,
            id: session.id,
            returned: false,
        };
        session.settle(session.subscribe_from_start(), &wait).await;
        let screen = rendered_after_wait(&session, &wait, &args);
        registration.returned = true;
        Ok(screen)
    }

    async fn send(&self, args: TerminalArgs) -> Result<String, String> {
        let session = self.session(args.id)?;
        if !session.is_running() {
            return Ok(format!(
                "{}\nThe program has exited; close the session.",
                session.render(scrollback_lines(&args))
            ));
        }
        let keys = args.keys.as_deref().unwrap_or_default();
        let segments = input_segments(args.text.as_deref().unwrap_or_default(), keys, session.input_modes())?;
        if segments.is_empty() {
            return Err("send needs text or keys".to_string());
        }
        let wait = wait_for(&args);
        let output = session.subscribe();
        session.write_segments(segments).await?;
        session.settle(output, &wait).await;
        Ok(rendered_after_wait(&session, &wait, &args))
    }

    async fn resize(&self, args: &TerminalArgs) -> Result<String, String> {
        let session = self.session(args.id)?;
        let (Some(cols), Some(rows)) = (args.cols, args.rows) else {
            return Err("resize needs cols and rows".to_string());
        };
        session.resize(TermSize::new(cols, rows)?).await?;
        Ok(session.render(scrollback_lines(args)))
    }

    async fn close(&self, id: Option<u32>) -> Result<String, String> {
        let session = self.session(id)?;
        lock_sessions(&self.sessions).live.remove(&session.id);
        let summary = session.summary();
        let ended = tokio::task::spawn_blocking(move || session.terminate())
            .await
            .map_err(|e| format!("closing the terminal failed: {e}"))?;
        Ok(format!("{summary}\nClosed: {ended}."))
    }

    fn list(&self) -> String {
        let sessions = lock_sessions(&self.sessions);
        if sessions.live.is_empty() {
            return "No terminal sessions.".to_string();
        }
        let lines: Vec<String> = sessions.live.values().map(|s| s.summary()).collect();
        lines.join("\n")
    }

    fn session(&self, id: Option<u32>) -> Result<Arc<TerminalSession>, String> {
        let id = id.ok_or("this action needs the session id")?;
        lock_sessions(&self.sessions)
            .live
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no terminal session {id}; use action 'list' to see open sessions"))
    }
}

fn wait_for(args: &TerminalArgs) -> Wait {
    Wait::new(
        Duration::from_millis(args.wait_ms.unwrap_or(DEFAULT_WAIT_MS).min(MAX_WAIT_MS)),
        args.wait_for.clone().filter(|text| !text.is_empty()),
    )
}

fn scrollback_lines(args: &TerminalArgs) -> usize {
    args.scrollback
        .map_or(0, |lines| usize::from(lines).min(MAX_SCROLLBACK))
}

fn rendered_after_wait(session: &TerminalSession, wait: &Wait, args: &TerminalArgs) -> String {
    let screen = session.render(scrollback_lines(args));
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
