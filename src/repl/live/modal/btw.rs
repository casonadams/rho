use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::engine::{AgentEngine, EphemeralTurnRequest};
use rho_harness_core::model::ChatMessage;
use tokio::sync::mpsc;

use super::{ModalKeyResult, apply_input_edit};
use crate::engine::runner::CancellationSignal;
use crate::error::Result;
use crate::ui::interactive::{
    InputAction, ModalState, TerminalBackend, TerminalController, UiAction, map_key, modal_body_max_scroll,
};

pub const BTW_MODAL_TITLE: &str = "Side Question (/btw)";
const BTW_EMPTY_HINT: &str = "Ask an ephemeral side question (history is not saved).";
const BTW_PAGE_SCROLL_STEP: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtwStreamEvent {
    Delta(String),
    Done,
    Error(String),
}

pub struct BtwSession {
    pub turns: Vec<(String, String)>,
    in_flight_cancel: Option<CancellationSignal>,
    pub streaming: bool,
    partial_assistant: Option<String>,
    delta_rx: Option<mpsc::UnboundedReceiver<BtwStreamEvent>>,
}

impl BtwSession {
    pub fn new(partial_assistant: Option<String>) -> Self {
        Self {
            turns: Vec::new(),
            in_flight_cancel: None,
            streaming: false,
            partial_assistant,
            delta_rx: None,
        }
    }

    pub fn build_modal_body(&self) -> String {
        if self.turns.is_empty() {
            return BTW_EMPTY_HINT.to_string();
        }
        let last_idx = self.turns.len() - 1;
        let mut sections = Vec::with_capacity(self.turns.len());
        for (idx, (question, answer)) in self.turns.iter().enumerate() {
            let is_streaming_turn = idx == last_idx && self.streaming;
            sections.push(format_btw_turn(question, answer, is_streaming_turn));
        }
        sections.join("\n\n")
    }

    pub fn extra_messages(&self) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(self.turns.len() * 2);
        for (question, answer) in &self.turns {
            if answer.is_empty() {
                continue;
            }
            messages.push(ChatMessage::user(question.clone()));
            messages.push(ChatMessage::assistant(answer.clone()));
        }
        messages
    }

    pub async fn submit_question(&mut self, engine: &AgentEngine, question: String) {
        self.cancel();
        let extra_messages = self.extra_messages();
        self.turns.push((question.clone(), String::new()));

        let cancel = CancellationSignal::default();
        self.in_flight_cancel = Some(cancel.clone());
        self.streaming = true;

        let req = EphemeralTurnRequest {
            prompt: question,
            extra_messages,
            partial_assistant: self.partial_assistant.clone(),
            max_tokens: None,
            cancellation: cancel,
        };

        match engine.prepare_ephemeral_turn(req).await {
            Ok(prepared) => {
                let (event_tx, event_rx) = mpsc::unbounded_channel();
                self.delta_rx = Some(event_rx);
                spawn_btw_stream(prepared, event_tx);
            }
            Err(err) => {
                self.finish_with_error(err.to_string());
            }
        }
    }

    pub fn poll_updates(&mut self) -> bool {
        let mut changed = false;
        let mut finished = false;
        let mut disconnected = false;
        let mut pending_error: Option<String> = None;

        if let Some(rx) = self.delta_rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(BtwStreamEvent::Delta(chunk)) => {
                        append_delta(&mut self.turns, &chunk);
                        changed = true;
                    }
                    Ok(BtwStreamEvent::Done) => {
                        finished = true;
                        changed = true;
                        break;
                    }
                    Ok(BtwStreamEvent::Error(msg)) => {
                        pending_error = Some(msg);
                        finished = true;
                        changed = true;
                        break;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if let Some(msg) = pending_error {
            append_error(&mut self.turns, &msg);
        }
        if finished || disconnected {
            self.streaming = false;
            self.in_flight_cancel = None;
            self.delta_rx = None;
        }
        changed
    }

    pub fn cancel(&mut self) {
        if let Some(sig) = self.in_flight_cancel.take() {
            sig.cancel();
        }
        self.streaming = false;
        self.delta_rx = None;
    }

    pub fn last_answer(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .map(|(_, answer)| answer.as_str())
            .find(|answer| !answer.trim().is_empty())
    }

    fn finish_with_error(&mut self, msg: String) {
        append_error(&mut self.turns, &msg);
        self.streaming = false;
        self.in_flight_cancel = None;
        self.delta_rx = None;
    }
}

fn format_btw_turn(question: &str, answer: &str, is_streaming: bool) -> String {
    let header = format!("> {question}");
    if answer.is_empty() && is_streaming {
        format!("{header}\n...")
    } else if is_streaming {
        format!("{header}\n{answer} \u{258c}")
    } else if answer.is_empty() {
        header
    } else {
        format!("{header}\n{answer}")
    }
}

fn append_delta(turns: &mut [(String, String)], chunk: &str) {
    if let Some((_, answer)) = turns.last_mut() {
        answer.push_str(chunk);
    }
}

fn append_error(turns: &mut [(String, String)], msg: &str) {
    if let Some((_, answer)) = turns.last_mut() {
        if answer.is_empty() {
            *answer = format!("Error: {msg}");
        } else {
            answer.push_str(&format!("\n\nError: {msg}"));
        }
    }
}

fn spawn_btw_stream(
    prepared: rho_engine::engine::PreparedEphemeralTurn,
    event_tx: mpsc::UnboundedSender<BtwStreamEvent>,
) {
    tokio::spawn(async move {
        let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
        let exec_fut = prepared.execute(Some(delta_tx));
        tokio::pin!(exec_fut);
        let result = loop {
            tokio::select! {
                biased;
                Some(chunk) = delta_rx.recv() => {
                    let _ = event_tx.send(BtwStreamEvent::Delta(chunk));
                }
                res = &mut exec_fut => break res,
            }
        };
        while let Ok(chunk) = delta_rx.try_recv() {
            let _ = event_tx.send(BtwStreamEvent::Delta(chunk));
        }
        match result {
            Ok(_) => {
                let _ = event_tx.send(BtwStreamEvent::Done);
            }
            Err(err) => {
                let _ = event_tx.send(BtwStreamEvent::Error(err.to_string()));
            }
        }
    });
}

fn scroll_modal_to_bottom<B: TerminalBackend>(modal: &mut ModalState, controller: &TerminalController<B>) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    modal.body_scroll = modal_body_max_scroll(modal, &draft, w, h);
}

pub fn open_btw_modal<B: TerminalBackend>(controller: &mut TerminalController<B>, body: String) {
    let mut modal = ModalState::new(BTW_MODAL_TITLE, body, Vec::new());
    modal.enter_input_mode("btw> ");
    scroll_modal_to_bottom(&mut modal, controller);
    controller.state_mut().push_modal(modal);
}

fn sync_btw_modal_body<B: TerminalBackend>(controller: &mut TerminalController<B>, body: String) {
    let is_open = controller
        .state()
        .active_modal()
        .is_some_and(|m| m.title == BTW_MODAL_TITLE);
    if !is_open {
        return;
    }
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.body = body;
        modal.body_scroll = modal_body_max_scroll(modal, &draft, w, h);
    }
}

pub fn refresh_btw_modal<B: TerminalBackend>(controller: &mut TerminalController<B>, btw: &mut BtwSession) -> bool {
    if !btw.poll_updates() {
        return false;
    }
    let body = btw.build_modal_body();
    let is_open = controller
        .state()
        .active_modal()
        .is_some_and(|m| m.title == BTW_MODAL_TITLE);
    if !is_open {
        return false;
    }
    sync_btw_modal_body(controller, body);
    true
}

pub(crate) async fn start_btw_overlay<B: TerminalBackend>(
    session: &mut crate::repl::ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
    question: Option<String>,
    partial_assistant: Option<String>,
) {
    let mut btw = BtwSession::new(partial_assistant);
    if let Some(q) = question {
        btw.submit_question(engine, q).await;
    }
    let body = btw.build_modal_body();
    session.btw = Some(btw);
    open_btw_modal(controller, body);
}

pub(crate) async fn handle_btw_modal_action<B: TerminalBackend>(
    session: &mut crate::repl::ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
    action: &ModalKeyResult,
) -> bool {
    match action {
        ModalKeyResult::BtwSubmit(question) => {
            let btw = session.btw.get_or_insert_with(|| BtwSession::new(None));
            btw.submit_question(engine, question.clone()).await;
            let body = btw.build_modal_body();
            sync_btw_modal_body(controller, body);
            true
        }
        ModalKeyResult::BtwCopy => {
            if let Some(answer) = session.btw.as_ref().and_then(BtwSession::last_answer)
                && crate::platform::clipboard::set_text(answer).is_ok()
            {
                controller.set_system_message("Copied /btw answer to clipboard");
            }
            true
        }
        ModalKeyResult::BtwClose => {
            if let Some(mut btw) = session.btw.take() {
                btw.cancel();
            }
            true
        }
        _ => false,
    }
}

fn scroll_btw_body<B: TerminalBackend>(controller: &mut TerminalController<B>, delta: isize) {
    let (w, h) = (controller.terminal_width(), controller.terminal_height());
    let draft = controller.state().editor().text().to_string();
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let max_scroll = modal_body_max_scroll(modal, &draft, w, h);
        modal.clamp_body_scroll(max_scroll);
        if delta < 0 {
            modal.body_scroll = modal.body_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            modal.body_scroll = (modal.body_scroll + delta.unsigned_abs()).min(max_scroll);
        }
    }
}

fn is_newline_key(key: &KeyEvent) -> bool {
    (key.code == KeyCode::Enter
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL))
        || (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL))
        || key.code == KeyCode::Char('\n')
}

fn handle_btw_edit_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent) {
    match map_key(key) {
        InputAction::Clear => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                modal.input.set_text("");
            }
        }
        InputAction::ClipboardPasteImage => {
            if let Some(text) = crate::platform::clipboard::get_text_or_image_path()
                && let Some(modal) = controller.state_mut().active_modal_mut()
            {
                apply_input_edit(&mut modal.input, UiAction::Paste(text));
            }
        }
        InputAction::Edit(action) => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                apply_input_edit(&mut modal.input, action);
            }
        }
        _ => {}
    }
}

pub fn handle_btw_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Esc {
        controller.state_mut().pop_modal();
        controller.redraw()?;
        return Ok(ModalKeyResult::BtwClose);
    }
    if key.code == KeyCode::Char('y') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(ModalKeyResult::BtwCopy);
    }
    if handle_btw_scroll_key(controller, key.code) {
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    if is_newline_key(&key) {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            apply_input_edit(&mut modal.input, UiAction::InsertNewline);
        }
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    if key.code == KeyCode::Enter {
        return submit_btw_enter(controller);
    }
    handle_btw_edit_key(controller, key);
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn handle_btw_scroll_key<B: TerminalBackend>(controller: &mut TerminalController<B>, code: KeyCode) -> bool {
    let step = match code {
        KeyCode::Up => -1,
        KeyCode::Down => 1,
        KeyCode::PageUp => -(BTW_PAGE_SCROLL_STEP as isize),
        KeyCode::PageDown => BTW_PAGE_SCROLL_STEP as isize,
        _ => return false,
    };
    scroll_btw_body(controller, step);
    true
}

fn submit_btw_enter<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let question = controller
        .state()
        .active_modal()
        .map(|m| m.input.expanded_text().trim().to_string())
        .unwrap_or_default();
    if question.is_empty() {
        return Ok(ModalKeyResult::Handled);
    }
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.input.set_text("");
    }
    controller.redraw()?;
    Ok(ModalKeyResult::BtwSubmit(question))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::{InteractiveState, NullBackend};

    fn test_controller() -> TerminalController<NullBackend> {
        TerminalController::new(NullBackend, InteractiveState::default()).unwrap()
    }

    #[test]
    fn btw_session_builds_body_and_extra_messages() {
        let mut session = BtwSession::new(None);
        assert_eq!(session.build_modal_body(), BTW_EMPTY_HINT);
        assert!(session.extra_messages().is_empty());

        session.turns.push(("First question".into(), "First answer".into()));
        session.turns.push(("Second question".into(), String::new()));
        session.streaming = true;

        let body = session.build_modal_body();
        assert!(body.contains("> First question\nFirst answer"));
        assert!(body.contains("> Second question\n..."));

        let extra = session.extra_messages();
        assert_eq!(
            extra,
            vec![
                ChatMessage::user("First question"),
                ChatMessage::assistant("First answer")
            ]
        );
    }

    #[test]
    fn btw_session_poll_updates_accumulates_deltas_and_finishes() {
        let mut session = BtwSession::new(None);
        session.turns.push(("What?".into(), String::new()));
        session.streaming = true;
        let (tx, rx) = mpsc::unbounded_channel();
        session.delta_rx = Some(rx);

        tx.send(BtwStreamEvent::Delta("Hello".into())).unwrap();
        tx.send(BtwStreamEvent::Delta(" world".into())).unwrap();
        assert!(session.poll_updates());
        assert_eq!(session.last_answer(), Some("Hello world"));
        assert!(session.streaming);

        tx.send(BtwStreamEvent::Done).unwrap();
        assert!(session.poll_updates());
        assert!(!session.streaming);
        assert!(!session.poll_updates());
    }

    #[test]
    fn btw_session_cancel_only_cancels_btw_signal() {
        let main_cancel = CancellationSignal::default();
        let btw_cancel = CancellationSignal::default();
        let mut session = BtwSession::new(None);
        session.in_flight_cancel = Some(btw_cancel.clone());
        session.streaming = true;

        session.cancel();
        assert!(btw_cancel.is_cancelled());
        assert!(!main_cancel.is_cancelled());
        assert!(!session.streaming);
    }

    #[test]
    fn btw_modal_keys_submit_scroll_clear_and_close() {
        let mut controller = test_controller();
        let long_body = (1..=40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        open_btw_modal(&mut controller, long_body);

        let initial_scroll = controller.state().active_modal().unwrap().body_scroll;
        assert!(initial_scroll > 0);

        let res = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Up)).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);
        assert!(controller.state().active_modal().unwrap().body_scroll < initial_scroll);

        let res = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Down)).unwrap();
        assert_eq!(res, ModalKeyResult::Handled);
        assert_eq!(controller.state().active_modal().unwrap().body_scroll, initial_scroll);

        let _ = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Char('h'))).unwrap();
        let _ = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Char('i'))).unwrap();
        assert_eq!(controller.state().active_modal().unwrap().input.text(), "hi");

        let res = handle_btw_key(
            &mut controller,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(res, ModalKeyResult::Handled);
        assert_eq!(controller.state().active_modal().unwrap().input.text(), "");

        let _ = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Char('q'))).unwrap();
        let res = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Enter)).unwrap();
        assert_eq!(res, ModalKeyResult::BtwSubmit("q".into()));
        assert_eq!(controller.state().active_modal().unwrap().input.text(), "");

        let res = handle_btw_key(&mut controller, KeyEvent::from(KeyCode::Esc)).unwrap();
        assert_eq!(res, ModalKeyResult::BtwClose);
        assert!(controller.state().active_modal().is_none());
    }
}
