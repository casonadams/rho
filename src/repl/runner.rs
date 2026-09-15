use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::sync::Arc;
use std::time::Duration;

use rho_harness_core::presentation::WelcomeDisplay;
use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::session::list_session_summaries_async;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::modal::{McpModalState, ModelRegistry, SettingsState, SkillModalState};
use rho_ui_core::session::PROVIDER_DEFS;
use rho_ui_core::state::RhoTicket;

use crate::engine::AgentEngine;
use crate::engine::runner::{CancellationSignal, TurnRequest};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::interactive::InteractiveHistory;
use crate::ui::editor::{EditorMode, TextAreaEditor};
use crate::ui::modal::{AutocompletePopupView, RemotePairModalView, StandardModalView};
use crate::ui::terminal::TerminalRunner;
use crate::ui::{ModalView, PromptEditor, TerminalComponent, TerminalSurface};

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

enum ActiveModal {
    Standard(StandardModalView),
    RemotePair(RemotePairModalView),
}

impl ModalView for ActiveModal {
    fn title(&self) -> &str {
        match self {
            Self::Standard(m) => m.title(),
            Self::RemotePair(m) => m.title(),
        }
    }

    fn selected_index(&self) -> usize {
        match self {
            Self::Standard(m) => m.selected_index(),
            Self::RemotePair(m) => m.selected_index(),
        }
    }

    fn item_count(&self) -> usize {
        match self {
            Self::Standard(m) => m.item_count(),
            Self::RemotePair(m) => m.item_count(),
        }
    }

    fn filter(&self) -> &str {
        match self {
            Self::Standard(m) => m.filter(),
            Self::RemotePair(m) => m.filter(),
        }
    }

    fn set_filter(&mut self, query: &str) {
        match self {
            Self::Standard(m) => m.set_filter(query),
            Self::RemotePair(m) => m.set_filter(query),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self {
            Self::Standard(m) => m.handle_key(key),
            Self::RemotePair(m) => m.handle_key(key),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Self::Standard(m) => m.render(frame, area),
            Self::RemotePair(m) => m.render(frame, area),
        }
    }
}

impl ActiveModal {
    fn is_open(&self) -> bool {
        match self {
            Self::Standard(m) => m.state.is_open,
            Self::RemotePair(m) => m.is_open,
        }
    }
}

struct RunnerState<'a> {
    pub session: &'a mut ReplSession,
    pub engine: &'a mut AgentEngine,
    pub runner: &'a mut TerminalRunner<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    pub editor: &'a mut TextAreaEditor,
    pub history: &'a mut InteractiveHistory,
    pub active_modal: &'a mut Option<ActiveModal>,
    pub autocomplete_popup: &'a mut Option<AutocompletePopupView>,
}

async fn init_live_engine(session: &mut ReplSession) -> Result<AgentEngine> {
    let engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;

    if let Some(ref cli) = session.cli
        && let Some(ref name) = cli.name
    {
        let _ = engine.session_manager.set_session_name(name).await;
    }

    session.config = engine.config.clone();
    engine.refresh_quota().await;
    Ok(engine)
}

async fn print_startup_banner(session: &ReplSession, engine: &AgentEngine) {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let tools = engine.tool_names();
    let mcp = session.config.mcp.servers.keys().cloned().collect::<Vec<_>>();
    let agents = engine.instruction_files().await;
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let display = WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        mcp,
    };
    session.renderer.print_welcome(&display);
}

fn build_completions() -> CompletionEngine {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let mut completions = CompletionEngine::new();
    for skill in &skills {
        completions
            .skills
            .push((skill.metadata.name.clone(), skill.metadata.description.clone()));
    }
    for model in &ModelRegistry::default().models {
        completions.models.push((model.id.clone(), model.display_name.clone()));
    }
    completions
}

async fn load_history(session: &ReplSession) -> InteractiveHistory {
    let history_path = session.config.config_dir.join("history.txt");
    InteractiveHistory::with_file_async(1000, history_path)
        .await
        .unwrap_or_else(|_| {
            InteractiveHistory::with_file(1000, std::env::temp_dir().join("rho_fallback_hist.txt"))
                .expect("fallback history")
        })
}

fn render_top_bar(f: &mut Frame, area: Rect, editor: &TextAreaEditor) {
    let mode_badge = editor.mode_label();
    let top_title = if mode_badge.is_empty() {
        format!(" rho v{} ", env!("CARGO_PKG_VERSION"))
    } else {
        format!(" [{mode_badge}] rho v{} ", env!("CARGO_PKG_VERSION"))
    };
    let top_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(top_title);
    f.render_widget(top_block, area);
}

fn render_footer(f: &mut Frame, area: Rect, engine: &AgentEngine) {
    let model_name = &engine.config.model;
    let thinking = engine.config.thinking_level.as_deref().unwrap_or("default");
    let usage = engine.session_usage_totals();
    let tokens_fmt = rho_harness_core::tokens::format_tokens(usage.total_input + usage.total_output);
    let current_dir = std::env::current_dir().unwrap_or_default();
    let cwd = rho_ui_core::footer::abbreviate_home(&current_dir, None);
    let branch = rho_ui_core::footer::get_git_branch(&current_dir);
    let branch_str = branch.as_deref().unwrap_or("");
    let left = format!(" {model_name} · {thinking} · {tokens_fmt}");
    let right = if branch_str.is_empty() {
        format!("{cwd} ")
    } else {
        format!("{cwd} ({branch_str}) ")
    };
    let pad = (area.width as usize).saturating_sub(left.len() + right.len());
    let footer_text = format!("{left}{}{right}", " ".repeat(pad));
    let footer_para = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer_para, area);
}

fn render_viewport(state: &mut RunnerState<'_>) -> Result<()> {
    state.runner.draw(|f| {
        let area = f.area();
        if let Some(modal) = state.active_modal.as_ref() {
            modal.render(f, area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        render_top_bar(f, chunks[0], state.editor);
        state.editor.render(f, chunks[1]);
        render_footer(f, chunks[2], state.engine);

        if let Some(popup) = state.autocomplete_popup.as_ref() {
            popup.render_anchored(f, chunks[1], area);
        }
    })?;
    Ok(())
}

fn apply_modal_selection(modal: &ActiveModal, session: &mut ReplSession, _engine: &mut AgentEngine) {
    if let ActiveModal::Standard(std_modal) = modal
        && let Some(opt) = std_modal.state.selected_option()
    {
        let val = opt.value.clone();
        match std_modal.state.title.as_str() {
            "Select Model" => {
                session.config.model = val;
            }
            "Select Thinking Level" => {
                session.config.thinking_level = Some(val);
            }
            _ => {}
        }
    }
}

fn handle_modal_input(
    mut modal: ActiveModal,
    key: KeyEvent,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Option<ActiveModal> {
    if key.code == KeyCode::Enter {
        apply_modal_selection(&modal, session, engine);
        None
    } else {
        modal.handle_key(key);
        if modal.is_open() { Some(modal) } else { None }
    }
}

fn handle_popup_input(
    mut popup: AutocompletePopupView,
    key: KeyEvent,
    editor: &mut TextAreaEditor,
) -> (Option<AutocompletePopupView>, bool) {
    match key.code {
        KeyCode::Enter | KeyCode::Tab if key.modifiers.is_empty() => {
            if let Some(cand) = popup.selected() {
                editor.set_text(&cand.value);
            }
            (None, true)
        }
        KeyCode::Esc => (None, true),
        KeyCode::Down => {
            popup.select_next();
            (Some(popup), true)
        }
        KeyCode::Up => {
            popup.select_prev();
            (Some(popup), true)
        }
        _ => (None, false),
    }
}

async fn handle_key_cycle(state: &mut RunnerState<'_>, key: KeyEvent, completions: &CompletionEngine) -> Result<bool> {
    if let Some(modal) = state.active_modal.take() {
        *state.active_modal = handle_modal_input(modal, key, state.session, state.engine);
        state.session.sync_engine_model(state.engine).await;
        return Ok(false);
    }

    if let Some(popup) = state.autocomplete_popup.take() {
        let (next_popup, consumed) = handle_popup_input(popup, key, state.editor);
        *state.autocomplete_popup = next_popup;
        if consumed {
            return Ok(false);
        }
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        if !state.editor.is_empty() {
            state.editor.clear();
        }
        return Ok(false);
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
        return Ok(state.editor.is_empty());
    }

    if !state.editor.handle_key(key) {
        let prompt = state.editor.text().to_string();
        state.editor.clear();
        return handle_submission(state, prompt).await;
    }

    let text = state.editor.text();
    if text.starts_with('/') && !text.contains(' ') {
        let candidates = completions.complete(&text, text.len());
        *state.autocomplete_popup = (!candidates.is_empty()).then(|| AutocompletePopupView::new(candidates));
    } else {
        *state.autocomplete_popup = None;
    }
    Ok(false)
}

pub async fn run_unified_live(session: &mut ReplSession) -> Result<()> {
    let mut engine = init_live_engine(session).await?;
    let mut runner = TerminalRunner::from_stdout(10)?;
    print_startup_banner(session, &engine).await;

    let mode = if session.config.editor.is_vim() {
        EditorMode::Vim
    } else {
        EditorMode::Default
    };
    let mut editor = TextAreaEditor::new(mode);
    let mut history = load_history(session).await;
    let completions = build_completions();

    let mut active_modal: Option<ActiveModal> = None;
    let mut autocomplete_popup: Option<AutocompletePopupView> = None;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(50));

    loop {
        let mut state = RunnerState {
            session,
            engine: &mut engine,
            runner: &mut runner,
            editor: &mut editor,
            history: &mut history,
            active_modal: &mut active_modal,
            autocomplete_popup: &mut autocomplete_popup,
        };

        render_viewport(&mut state)?;

        tokio::select! {
            _ = ticker.tick() => {}
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Resize(_, _) => {
                        let _ = runner.clear();
                    }
                    Event::Key(key) => {
                        let mut state = RunnerState {
                            session,
                            engine: &mut engine,
                            runner: &mut runner,
                            editor: &mut editor,
                            history: &mut history,
                            active_modal: &mut active_modal,
                            autocomplete_popup: &mut autocomplete_popup,
                        };
                        let should_exit = handle_key_cycle(&mut state, key, &completions).await?;
                        if should_exit {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn handle_interactive_command(
    cmd: &str,
    session: &mut ReplSession,
    engine: &AgentEngine,
    active_modal: &mut Option<ActiveModal>,
) -> bool {
    match cmd {
        "/model" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::model(
                &ModelRegistry::default(),
            )));
            true
        }
        "/thinking" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::thinking(
                session.config.thinking_level.as_deref(),
            )));
            true
        }
        "/login" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::auth(PROVIDER_DEFS, None)));
            true
        }
        "/mcp" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::mcp(&McpModalState::default())));
            true
        }
        "/skill" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::skill(
                &SkillModalState::default(),
            )));
            true
        }
        "/session" => {
            let summaries = list_session_summaries_async(&session.config.sessions_dir)
                .await
                .unwrap_or_default();
            *active_modal = Some(ActiveModal::Standard(StandardModalView::session(
                &summaries,
                Some(&engine.session_manager.session_id),
            )));
            true
        }
        "/pair" => {
            *active_modal = Some(ActiveModal::RemotePair(RemotePairModalView::new(RhoTicket {
                node_id: "rho-local-node".to_string(),
                relay_url: None,
            })));
            true
        }
        "/settings" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::settings(
                &SettingsState::default(),
            )));
            true
        }
        _ => false,
    }
}

async fn handle_slash_command(state: &mut RunnerState<'_>, cmd: &str, rest: &str) -> Result<bool> {
    if matches!(cmd, "/quit" | "/exit") {
        return Ok(true);
    }
    if cmd == "/clear" {
        let _ = state.runner.clear();
        return Ok(false);
    }
    if rest.is_empty() && handle_interactive_command(cmd, state.session, state.engine, state.active_modal).await {
        return Ok(false);
    }
    if cmd == "/model" && !rest.is_empty() {
        state.session.config.model = rest.to_string();
        state.session.sync_engine_model(state.engine).await;
        let _ = state.runner.commit_turn("", &format!("Switched model to {rest}\n"));
        return Ok(false);
    }
    if cmd == "/thinking" && !rest.is_empty() {
        state.session.config.thinking_level = Some(rest.to_string());
        state.session.sync_engine_model(state.engine).await;
        let _ = state.runner.commit_turn("", &format!("Set thinking level to {rest}\n"));
        return Ok(false);
    }

    let mut cmd_ctx = SlashCommandContext {
        config: &mut state.session.config,
        auth_store: &mut state.session.auth_store,
        renderer: &state.session.renderer,
        session_id: Some(&state.engine.session_manager.session_id),
        session_manager: Some(&state.engine.session_manager),
        engine: Some(state.engine),
        home_dir: None,
    };
    let trimmed = if rest.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {rest}")
    };
    let should_exit = matches!(
        SlashCommandHandler::handle(&trimmed, &mut cmd_ctx).await,
        Ok(Some(CommandResult::Exit))
    );
    Ok(should_exit)
}

async fn execute_agent_turn(state: &mut RunnerState<'_>, prompt: &str) -> Result<()> {
    let cancellation = Arc::new(CancellationSignal::default());
    let request = TurnRequest::new(prompt).with_cancellation(&cancellation);
    let broadcast: Arc<dyn rho_harness_core::presentation::Presenter> =
        Arc::new(crate::ui::render::BroadcastPresenter::new(
            Arc::new(state.session.renderer.clone()),
            crate::platform::remote::PEER_REGISTRY.clone(),
        ));

    let _ = state.runner.commit_turn(&format!("> {prompt}"), "");

    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::TurnStart {
        turn_number: 1,
        prompt: prompt.to_string(),
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::StatusChanged {
        status: "busy".to_string(),
    });

    let turn_res = state.engine.run_turn(request, broadcast).await;
    state.session.sync_engine_model(state.engine).await;
    state.engine.refresh_quota().await;

    let totals = state.engine.session_usage_totals();
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: None,
        context_window: None,
        tokens_per_second: None,
        quota: None,
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::StatusChanged {
        status: "idle".to_string(),
    });

    match turn_res {
        Ok(out) => {
            let _ = state.runner.commit_turn("", &out.final_text);
        }
        Err(err) => {
            let _ = state.runner.commit_turn("", &format!("\nError: {err}\n"));
        }
    }

    Ok(())
}

async fn handle_submission(state: &mut RunnerState<'_>, prompt: String) -> Result<bool> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let _ = state.history.record(trimmed);

    if trimmed.starts_with('/') {
        let (cmd, rest) = match trimmed.split_once(' ') {
            Some((c, r)) => (c, r.trim()),
            None => (trimmed, ""),
        };
        return handle_slash_command(state, cmd, rest).await;
    }

    execute_agent_turn(state, trimmed).await?;
    Ok(false)
}
