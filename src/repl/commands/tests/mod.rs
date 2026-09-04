use super::*;
use crate::config::Config;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rho_engine::auth::AuthStore;
use tokio::sync::mpsc;

pub(super) fn collecting_renderer() -> (TerminalRenderer, mpsc::UnboundedReceiver<UiEvent>) {
    let (ui, events) = InteractiveUi::channel();
    (TerminalRenderer::with_ui(ui), events)
}

pub(super) fn collected_output(events: &mut mpsc::UnboundedReceiver<UiEvent>) -> String {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Output(OutputEvent::Text(text)) => Some(text),
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect()
}

pub(super) fn test_context<'a>(
    config: &'a mut Config,
    auth_store: &'a mut AuthStore,
    renderer: &'a TerminalRenderer,
) -> SlashCommandContext<'a> {
    SlashCommandContext {
        config,
        auth_store,
        renderer,
        session_id: None,
        session_manager: None,
        engine: None,
        home_dir: None,
    }
}

mod dispatch;
mod export;
mod session;
mod skills;
