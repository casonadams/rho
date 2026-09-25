pub mod autocomplete;
pub mod bash_runner;
pub mod batch;
pub mod idle;
pub mod message;
pub mod modal;
pub mod navigation;
mod setup;
#[cfg(test)]
mod tests;
pub mod turn;
pub mod types;

pub use types::*;

use batch::drain_ui_events;
use idle::read_idle_input;
use navigation::update_footer;
use setup::{build_completions, display_welcome_banner, init_live_engine, init_live_ui, maybe_hydrate_transcript};

use super::ReplSession;
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::interactive::{CrosstermBackend, QueuedMessage, TerminalBackend, TerminalController, UiEvent};

async fn next_live_message<B: TerminalBackend>(
    io: LiveIo<'_, B>,
    editor: EditorResources<'_>,
    (session, engine): (&mut ReplSession, &mut AgentEngine),
) -> Result<Option<QueuedMessage>> {
    if let Some(msg) = io.controller.state_mut().pop_queued() {
        return Ok(Some(msg));
    }
    read_idle_input(IdleContext {
        io,
        editor,
        session,
        engine,
    })
    .await
}

async fn dispatch_live_message<B: TerminalBackend>(
    (io, editor): (&mut LiveIo<'_, B>, &mut EditorResources<'_>),
    (session, engine): (&mut ReplSession, &mut AgentEngine),
    message: QueuedMessage,
) -> Result<bool> {
    editor
        .history
        .record(&message.text)
        .map_err(|e| anyhow::anyhow!("History could not be updated: {e}"))?;
    let io = LiveIo {
        controller: io.controller,
        events: io.events,
        input: io.input,
    };
    let editor = EditorResources {
        history: editor.history,
        completions: editor.completions,
    };
    session
        .process_live_message(engine, LiveMessage { io, editor, message })
        .await
}

async fn handle_live_message<B: TerminalBackend>(
    (io, editor): (&mut LiveIo<'_, B>, &mut EditorResources<'_>),
    (session, engine): (&mut ReplSession, &mut AgentEngine),
    msg: QueuedMessage,
) -> Result<bool> {
    let done = dispatch_live_message((io, editor), (session, engine), msg).await?;
    if !done {
        update_footer(io.controller.state_mut(), session, engine);
        io.controller.redraw()?;
    }
    Ok(done)
}

async fn run_live_loop<B: TerminalBackend>(
    mut io: LiveIo<'_, B>,
    mut editor: EditorResources<'_>,
    (session, engine): (&mut ReplSession, &mut AgentEngine),
) -> Result<()> {
    loop {
        let Some(msg) = next_live_message(
            LiveIo {
                controller: io.controller,
                events: io.events,
                input: io.input,
            },
            EditorResources {
                history: editor.history,
                completions: editor.completions,
            },
            (session, engine),
        )
        .await?
        else {
            break;
        };
        if handle_live_message((&mut io, &mut editor), (session, engine), msg).await? {
            break;
        }
    }
    Ok(())
}

async fn build_live_environment(
    session: &mut ReplSession,
    engine: &AgentEngine,
) -> Result<(
    TerminalController<CrosstermBackend>,
    tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    InteractiveHistory,
    CompletionSet,
)> {
    let (mut controller, mut ui_events) = init_live_ui(session, engine)?;
    let skills = display_welcome_banner(session, engine).await;
    drain_ui_events(&mut controller, &mut ui_events, &mut None)?;
    let mut history = InteractiveHistory::with_file_async(1000, session.config.config_dir.join("history.txt"))
        .await
        .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?;
    let completions = build_completions(session, skills).await;
    maybe_hydrate_transcript(session.resume_id.is_some(), engine, (&mut controller, &mut history)).await;
    Ok((controller, ui_events, history, completions))
}

impl ReplSession {
    pub(super) async fn run_live(&mut self) -> Result<()> {
        let mut engine = init_live_engine(self).await?;
        let (mut controller, mut ui_events, mut history, completions) = build_live_environment(self, &engine).await?;
        let mut input = TerminalInputReader::spawn()?;
        let io = LiveIo {
            controller: &mut controller,
            events: &mut ui_events,
            input: &mut input,
        };
        let editor = EditorResources {
            history: &mut history,
            completions: &completions,
        };
        run_live_loop(io, editor, (self, &mut engine)).await
    }
}
