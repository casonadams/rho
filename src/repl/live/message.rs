//! Live message processing, bash escape expansion, and slash command dispatch.

use super::batch::drain_ui_events;
use super::turn::run_active_turn;
use super::{ActiveTurn, EditorResources, LiveIo, LiveMessage};
use crate::engine::AgentEngine;
use crate::error::{AppError, Result};
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use rho_harness_core::session::TreeNodeKind;

// ---------------------------------------------------------------------------
// Context structs
// ---------------------------------------------------------------------------

pub(super) struct LiveCommandContext<'a, 'b> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
}

struct SessionCommandIo<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub input: &'a mut TerminalInputReader,
}

struct BranchSwitchContext<'a, 'b, 'c, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'c mut TerminalController<B>,
    pub history: &'c mut InteractiveHistory,
    pub input: &'c mut TerminalInputReader,
}

// ---------------------------------------------------------------------------
// Bash escape expansion (! and !!)
// ---------------------------------------------------------------------------

async fn run_discarding_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let _ = super::bash_runner::run_user_bash(cmd, renderer, io).await;
    Ok(None)
}

async fn run_prompt_with_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let res = super::bash_runner::run_user_bash(cmd, renderer, io).await?;
    if res.is_cancelled {
        return Ok(None);
    }
    let status = if res.is_error { " (failed)" } else { "" };
    Ok(Some(format!(
        "Executed local shell command: `{cmd}`{status}\n\nOutput:\n```\n{}\n```",
        res.output
    )))
}

pub(super) async fn resolve_effective_prompt<B: TerminalBackend>(
    input: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    if let Some(cmd) = input.strip_prefix("!!").map(str::trim).filter(|c| !c.is_empty()) {
        return run_discarding_output(cmd, renderer, io).await;
    }
    if let Some(cmd) = input.strip_prefix('!').map(str::trim).filter(|c| !c.is_empty()) {
        return run_prompt_with_output(cmd, renderer, io).await;
    }
    Ok(Some(input.to_string()))
}

// ---------------------------------------------------------------------------
// Main message processing entry point
// ---------------------------------------------------------------------------

fn is_slash_input(input: &str) -> bool {
    crate::repl::commands::is_slash_command(input)
}

async fn run_slash_handler(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    input: &str,
) -> Result<Option<CommandResult>> {
    let mut command_context = SlashCommandContext {
        config: &mut session.config,
        auth_store: &mut session.auth_store,
        renderer: &session.renderer,
        session_id: Some(&engine.session_manager.session_id),
        session_manager: Some(&engine.session_manager),
        engine: Some(engine),
        home_dir: None,
    };
    SlashCommandHandler::handle(input, &mut command_context).await
}

async fn run_prompt_turn<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    live: LiveMessage<'_, B>,
    effective: String,
) -> Result<bool> {
    session.renderer.print_user_block(&effective);
    run_active_turn(
        session,
        engine,
        ActiveTurn {
            io: live.io,
            editor: live.editor,
            prompt: &effective,
        },
    )
    .await?;
    session.sync_engine_model(engine).await;
    engine.refresh_quota().await;
    Ok(false)
}

impl ReplSession {
    pub(super) async fn process_live_message<B: TerminalBackend>(
        &mut self,
        engine: &mut AgentEngine,
        mut live: LiveMessage<'_, B>,
    ) -> Result<bool> {
        let input = live.message.text.trim().to_string();
        if is_slash_input(&input)
            && let Some(result) = run_slash_handler(self, engine, &input).await?
        {
            return handle_live_command(LiveCommandContext { session: self, engine }, live, result).await;
        }

        let Some(effective) = resolve_effective_prompt(&input, &self.renderer, &mut live.io).await? else {
            drain_ui_events(live.io.controller, live.io.events, &mut None)?;
            return Ok(false);
        };

        run_prompt_turn(self, engine, live, effective).await
    }
}

// ---------------------------------------------------------------------------
// Slash command execution
// ---------------------------------------------------------------------------

async fn handle_selector_command(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut TerminalController<impl TerminalBackend>,
    action: &CommandResult,
) -> Result<()> {
    match action {
        CommandResult::OpenModelSelector => super::modal::open_model_selector(ctx.session, io_controller),
        CommandResult::OpenSettingsSelector => super::modal::open_settings_selector(io_controller),
        CommandResult::OpenThinkingSelector => super::modal::open_thinking_selector(ctx.session, io_controller),
        CommandResult::OpenLoginSelector => super::modal::open_login_selector(ctx.session, io_controller),
        _ => {}
    }
    io_controller.redraw()?;
    Ok(())
}

async fn handle_thinking_changed(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut TerminalController<impl TerminalBackend>,
    level: Option<&str>,
) {
    ctx.session.config.thinking_level = level.map(ToString::to_string);
    ctx.engine.config.thinking_level = ctx.session.config.thinking_level.clone();
    if let Err(err) = ctx.engine.update_model().await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not update thinking level: {err}\n"));
    }
    super::navigation::update_footer(io_controller.state_mut(), ctx.session, ctx.engine);
    let _ = io_controller.redraw();
}

async fn handle_model_changed(ctx: &mut LiveCommandContext<'_, '_>, new_model: &str, new_provider: Option<&str>) {
    ctx.session.config.model = new_model.to_string();
    if let Some(provider) = new_provider {
        ctx.session.config.provider = provider.to_string();
    }
    let provider = ctx.session.config.provider.clone();
    if let Err(err) = ctx.engine.switch_model(new_model, &provider).await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
    }
}

async fn handle_compact<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    instructions: Option<&str>,
) {
    ctx.session
        .renderer
        .print_notice("  [Compacting conversation context...]\n");
    match ctx.engine.compact_session(instructions).await {
        Ok(stats) => {
            let before = crate::ui::interactive::footer::format_tokens(stats.tokens_before as u64);
            let after = crate::ui::interactive::footer::format_tokens(stats.tokens_after as u64);
            let saved = crate::ui::interactive::footer::format_tokens(stats.saved_tokens as u64);
            ctx.session.renderer.print_notice(&format!(
                "  [Compacted context: {before} -> {after} tokens (saved {saved})]\n"
            ));
            super::turn::sync_turn_footer(io.controller, ctx.engine);
            let _ = io.controller.redraw();
        }
        Err(err) => {
            ctx.session
                .renderer
                .print_notice(&format!("  [Compaction failed: {err}]\n"));
        }
    }
}

async fn clear_engine_context(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine =
        crate::platform::agent_engine(ctx.session.config.clone(), ctx.session.auth_store.clone(), None).await?;
    Ok(())
}

async fn handle_model_or_thinking_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> bool {
    match result {
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            handle_model_changed(ctx, new_model, new_provider.as_deref()).await;
            true
        }
        CommandResult::ThinkingChanged { level } => {
            handle_thinking_changed(ctx, io.controller, level.as_deref()).await;
            true
        }
        _ => false,
    }
}

async fn handle_engine_command_rest<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    if handle_model_or_thinking_command(ctx, io, result).await {
        return Ok(true);
    }
    match result {
        CommandResult::Reload => {
            *ctx.engine = ctx.session.reload_engine(ctx.engine).await?;
            Ok(true)
        }
        CommandResult::Compact { instructions } => {
            handle_compact(ctx, io, instructions.as_deref()).await;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn handle_engine_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenModelSelector
        | CommandResult::OpenSettingsSelector
        | CommandResult::OpenThinkingSelector
        | CommandResult::OpenLoginSelector => {
            handle_selector_command(ctx, io.controller, result).await?;
        }
        CommandResult::ClearContext => clear_engine_context(ctx).await?,
        rest => return handle_engine_command_rest(ctx, io, rest).await,
    }
    Ok(true)
}

fn build_session_io<'a, 'b, B: TerminalBackend>(
    io: &'a mut LiveIo<'b, B>,
    history: &'a mut InteractiveHistory,
) -> SessionCommandIo<'a, B> {
    SessionCommandIo {
        controller: io.controller,
        history,
        input: io.input,
    }
}

fn flush_after_command<B: TerminalBackend>(io: &mut LiveIo<'_, B>) -> Result<()> {
    drain_ui_events(io.controller, io.events, &mut None)
}

pub(super) async fn handle_live_command<B: TerminalBackend>(
    mut ctx: LiveCommandContext<'_, '_>,
    live: LiveMessage<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    let LiveMessage {
        mut io,
        editor,
        message: _,
    } = live;

    let session_io = build_session_io(&mut io, editor.history);
    if handle_session_command(&mut ctx, session_io, result.clone()).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    if handle_auth_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    if handle_engine_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    run_live_command_tail(ctx, io, editor, result).await
}

async fn run_live_command_tail<B: TerminalBackend>(
    ctx: LiveCommandContext<'_, '_>,
    mut io: LiveIo<'_, B>,
    editor: EditorResources<'_>,
    result: CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::Exit => Ok(true),
        CommandResult::ExpandedPrompt { text } => {
            ctx.session.renderer.print_notice("  [Expanded template]\n");
            flush_after_command(&mut io)?;
            ctx.session.renderer.print_user_block(&text);
            run_active_turn(
                ctx.session,
                ctx.engine,
                ActiveTurn {
                    io,
                    editor,
                    prompt: &text,
                },
            )
            .await?;
            ctx.session.sync_engine_model(ctx.engine).await;
            ctx.engine.refresh_quota().await;
            Ok(false)
        }
        _ => flush_after_command(&mut io).map(|_| false),
    }
}

// ---------------------------------------------------------------------------
// Auth commands
// ---------------------------------------------------------------------------

fn handle_auth_result(ctx: &mut LiveCommandContext<'_, '_>, res: std::result::Result<(), AppError>, verb: &str) {
    match res {
        Ok(()) => {}
        Err(AppError::Cancelled(_)) => {}
        Err(err) => ctx.session.renderer.print_notice(&format!("  {verb} failed: {err}\n")),
    }
}

async fn rebuild_after_auth(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    Ok(())
}

async fn handle_auth_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::Login { provider } => {
            let login_res = io
                .suspend_for_async(|| {
                    crate::cli::login_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)
                })
                .await?;
            handle_auth_result(ctx, login_res, "Login");
            rebuild_after_auth(ctx).await?;
            Ok(true)
        }
        CommandResult::Logout { provider } => {
            let logout_res = io.suspend_for(|| {
                crate::cli::logout_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)
            })?;
            handle_auth_result(ctx, logout_res, "Logout");
            rebuild_after_auth(ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Session & Tree commands
// ---------------------------------------------------------------------------

async fn handle_tree_commands<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    let tree = ctx.engine.session_manager.load_tree().await?;
    match result {
        CommandResult::OpenTreeSelector => {
            super::modal::open_tree_selector(&tree, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::Tree => {
            let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
            ctx.session.renderer.print_notice(&format!(
                "\nConversation Tree (Session: {}):\n{rendered}\n",
                ctx.engine.session_manager.session_id
            ));
        }
        _ => {}
    }
    Ok(())
}

async fn handle_fork_session(ctx: &mut LiveCommandContext<'_, '_>, turn_or_node_id: &Option<String>) -> Result<()> {
    let forked = ctx
        .engine
        .session_manager
        .fork_session(&ctx.session.config.sessions_dir, turn_or_node_id.as_deref())
        .await?;
    ctx.session
        .renderer
        .print_status(&format!("Forked session: {}", forked.session_id));
    Ok(())
}

async fn handle_clone_session(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    let cloned = ctx
        .engine
        .session_manager
        .clone_session(&ctx.session.config.sessions_dir)
        .await?;
    ctx.session
        .renderer
        .print_status(&format!("Cloned session: {}", cloned.session_id));
    Ok(())
}

async fn handle_resume_session<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    session_id: &str,
    io: &mut SessionCommandIo<'_, B>,
) -> Result<()> {
    *ctx.engine = crate::platform::agent_engine(
        ctx.session.config.clone(),
        ctx.session.auth_store.clone(),
        Some(session_id),
    )
    .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = super::navigation::hydrate_session_transcript(io.controller, &tree, io.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}"));
    Ok(())
}

async fn switch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    match result {
        CommandResult::ForkSession { turn_or_node_id } => handle_fork_session(ctx, turn_or_node_id).await?,
        CommandResult::CloneSession => handle_clone_session(ctx).await?,
        CommandResult::ResumeSession { session_id } => handle_resume_session(ctx, session_id, io).await?,
        _ => {}
    }
    Ok(())
}

async fn handle_switch_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    if let CommandResult::SwitchBranch { leaf_id } = result {
        return switch_branch_for_command(ctx, io, leaf_id).await;
    }
    switch_session_result(ctx, io, result).await
}

async fn dispatch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    if is_tree_arm(result) {
        handle_tree_commands(ctx, io, result).await?;
        return Ok(true);
    }
    if is_session_switch_arm(result) {
        handle_switch_session_command(ctx, io, result).await?;
        return Ok(true);
    }
    handle_session_diagnostics(ctx, io, result).await
}

async fn switch_branch_for_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    leaf_id: &str,
) -> Result<()> {
    handle_switch_branch(
        BranchSwitchContext {
            session: ctx.session,
            engine: ctx.engine,
            controller: io.controller,
            history: io.history,
            input: io.input,
        },
        leaf_id.to_string(),
    )
    .await
}

async fn handle_session_diagnostics<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenSessionSelector => {
            super::modal::open_session_selector(&ctx.session.config.sessions_dir, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::NameSession { name } => {
            ctx.engine.session_manager.set_session_name(name).await?;
            ctx.session.renderer.print_status(&format!("Session name: \"{name}\""));
        }
        CommandResult::Rewind { turn } => {
            let retained_count = ctx.engine.session_manager.rewind_to_turn(*turn).await?;
            ctx.session.renderer.print_notice(&format!(
                "  [Rewound context to Turn {turn} ({retained_count} messages retained)]\n"
            ));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn handle_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    mut io: SessionCommandIo<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    dispatch_session_result(ctx, &mut io, &result).await
}

fn is_tree_arm(result: &CommandResult) -> bool {
    matches!(result, CommandResult::OpenTreeSelector | CommandResult::Tree)
}

fn is_session_switch_arm(result: &CommandResult) -> bool {
    matches!(
        result,
        CommandResult::SwitchBranch { .. }
            | CommandResult::ForkSession { .. }
            | CommandResult::CloneSession
            | CommandResult::ResumeSession { .. }
    )
}

// ---------------------------------------------------------------------------
// Branch switching
// ---------------------------------------------------------------------------

fn confirm_branch_summary() -> bool {
    inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
        .with_default(true)
        .prompt()
        .is_ok_and(|v| v)
}

async fn summarize_if_confirmed(ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>) -> Option<()> {
    let mut paused = ctx.input.pause().ok()?;
    paused.drain();
    ctx.controller.suspend().ok()?;
    let confirmed = confirm_branch_summary();
    let controller_res = ctx.controller.resume();
    let input_res = paused.resume();
    ctx.input.drain();
    controller_res.ok()?;
    input_res.ok()?;
    confirmed.then_some(())
}

fn abandoned_messages(abandoned: &[&rho_harness_core::session::tree::TreeNodeData]) -> Vec<rig::message::Message> {
    abandoned.iter().flat_map(|n| n.messages.clone()).collect()
}

async fn maybe_summarize_abandoned(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
) -> Option<String> {
    let has_assistant = abandoned.iter().any(|n| n.kind == TreeNodeKind::AssistantTurn);
    if !has_assistant || !ctx.session.renderer.has_interactive_ui() {
        return None;
    }
    summarize_if_confirmed(ctx).await?;
    Some(ctx.engine.summarize_branch(&abandoned_messages(abandoned)).await)
}

async fn rebuild_and_hydrate(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    leaf_id: &str,
) -> Result<()> {
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = super::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Switched active branch to {leaf_id}"));
    Ok(())
}

async fn switch_and_hydrate(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    leaf_id: &str,
    summary: Option<String>,
    old_leaf: &str,
) -> Result<()> {
    ctx.engine
        .session_manager
        .switch_branch(Some(leaf_id.to_string()))
        .await?;
    if let Some(summary) = summary.as_deref() {
        let _ = ctx
            .engine
            .session_manager
            .append_branch_summary(summary, old_leaf)
            .await;
    }
    rebuild_and_hydrate(ctx, leaf_id).await
}

async fn handle_switch_branch<B: TerminalBackend>(
    mut ctx: BranchSwitchContext<'_, '_, '_, B>,
    leaf_id: String,
) -> Result<()> {
    let old_leaf = ctx.engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = ctx.engine.session_manager.load_tree().await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = maybe_summarize_abandoned(&mut ctx, &abandoned).await;
    switch_and_hydrate(&mut ctx, &leaf_id, summary, &old_leaf).await
}
