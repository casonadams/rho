pub mod commands;
pub mod completer;
pub mod coordinator;
mod input_reader;
pub mod interactive;
mod live;
mod prompt;
#[cfg(test)]
mod tests;

pub use completer::RhoCompleter;
pub use prompt::SimplePrompt;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use crate::ui::TerminalRenderer;
use crate::ui::render::{SessionStatus, WelcomeDisplay};
use crossterm::QueueableCommand;
use crossterm::cursor::{MoveToColumn, MoveUp};
use crossterm::terminal::{Clear, ClearType};
use crossterm::tty::IsTty;
use reedline::{
    ColumnarMenu, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu,
    Signal, default_emacs_keybindings,
};
use std::io::Write;
use unicode_width::UnicodeWidthStr;

fn submitted_input_rows(input: &str, terminal_width: usize) -> u16 {
    let width = terminal_width.max(1);
    input.lines().fold(0_u16, |rows, line| {
        let occupied = UnicodeWidthStr::width(line).saturating_add(2);
        rows.saturating_add((occupied / width + 1).try_into().unwrap_or(u16::MAX))
    })
}

fn clear_submitted_input(input: &str) {
    let mut stdout = std::io::stdout();
    if !stdout.is_tty() {
        return;
    }
    let width = crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns))
        .unwrap_or(80);
    let rows = submitted_input_rows(input, width);
    let _ = stdout
        .queue(MoveUp(rows))
        .and_then(|stream| stream.queue(MoveToColumn(0)))
        .and_then(|stream| stream.queue(Clear(ClearType::FromCursorDown)))
        .and_then(Write::flush);
}

pub struct ReplSession {
    pub config: Config,
    pub auth_store: AuthStore,
    pub renderer: TerminalRenderer,
    pub resume_id: Option<String>,
    pub cli: Option<crate::config::cli::Cli>,
}

impl ReplSession {
    pub fn new(config: Config, auth_store: AuthStore, resume_id: Option<String>) -> Self {
        Self {
            config,
            auth_store,
            renderer: TerminalRenderer::default(),
            resume_id,
            cli: None,
        }
    }

    /// Retained so /reload can re-apply CLI overrides after re-reading config.
    pub fn with_cli(mut self, cli: Option<crate::config::cli::Cli>) -> Self {
        self.cli = cli;
        self
    }

    /// Re-read config (keeping CLI overrides and the runtime model choice),
    /// rebuild the engine, and preserve the session history.
    pub(crate) async fn reload_engine(&mut self, engine: &AgentEngine) -> Result<AgentEngine> {
        let mut config = Config::load(self.cli.as_ref())?;
        config.model = self.config.model.clone();
        config.provider = self.config.provider.clone();
        crate::repl::interactive::spawn_background_model_refresh(&config, &self.auth_store);
        let rebuilt = engine.rebuild(config.clone(), self.auth_store.clone()).await?;
        self.config = config;

        let skills: Vec<String> =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref())
                .into_iter()
                .map(|s| s.metadata.name)
                .collect();
        let tools = rebuilt.tool_names.clone();
        let mut plugins = self.config.plugins.keys().cloned().collect::<Vec<_>>();
        for mcp in self.config.mcp.servers.keys() {
            if !plugins.contains(mcp) {
                plugins.push(mcp.clone());
            }
        }

        self.renderer.print_notice(&format!(
            "  [Reloaded config, skills, and tools ({} skills, {} tools); session preserved]\n",
            skills.len(),
            tools.len()
        ));
        Ok(rebuilt)
    }

    pub async fn run(&mut self) -> Result<()> {
        let stdin_is_tty = std::io::stdin().is_tty();
        let stdout_is_tty = std::io::stdout().is_tty();
        if live::live_ui_supported(stdin_is_tty, stdout_is_tty) {
            return self.run_live().await;
        }

        self.run_interactive(stdin_is_tty).await
    }

    async fn run_interactive(&mut self, stdin_is_tty: bool) -> Result<()> {
        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        if let Some(ref cli) = self.cli
            && let Some(ref name) = cli.name
        {
            let _ = engine.session_manager.set_session_name(name).await;
        }
        self.config = engine.config.clone();
        engine.refresh_quota().await;

        let skills =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref());
        let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
        let tools = engine.tool_names.clone();
        let mut plugins = self.config.plugins.keys().cloned().collect::<Vec<_>>();
        for mcp in self.config.mcp.servers.keys() {
            if !plugins.contains(mcp) {
                plugins.push(mcp.clone());
            }
        }

        self.renderer.print_welcome(&WelcomeDisplay {
            model: self.config.model.clone(),
            provider: self.config.provider.clone(),
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
            tools,
            skills: skill_names,
            plugins,
        });

        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::ALT,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![reedline::EditCommand::InsertNewline]),
        );
        let edit_mode = Box::new(Emacs::new(keybindings));

        let skills =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref());
        let prompt_templates = rho_harness_core::prompts::discover_prompt_templates(
            Some(&self.config.config_dir),
            std::env::current_dir().ok().as_deref(),
        )
        .into_iter()
        .map(|t| t.metadata.name)
        .collect::<Vec<_>>();
        let models = crate::repl::interactive::discover_models(&self.config, &self.auth_store);
        let custom_providers = self.config.providers.keys().cloned().collect();
        let sources = crate::repl::interactive::CompletionSources::new()
            .with_skills(skills)
            .with_templates(prompt_templates)
            .with_models(models)
            .with_custom_providers(custom_providers);
        let completer = Box::new(RhoCompleter::new(sources));
        let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));

        let history = Box::new(
            FileBackedHistory::with_file(1000, self.config.config_dir.join("history.txt"))
                .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?,
        );

        let mut line_editor = Reedline::create()
            .with_history(history)
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_edit_mode(edit_mode);

        let prompt = SimplePrompt;
        let mut is_first_prompt = true;

        loop {
            if !is_first_prompt {
                self.renderer.write_output("\n");
            }
            is_first_prompt = false;

            let quota = engine.quota_display();
            self.renderer.print_session_status(&SessionStatus {
                model: self.config.model.clone(),
                provider: self.config.provider.clone(),
                context: engine.context_remaining_display(),
                quota: quota.clone(),
                auto_approve: self.config.auto_approve,
            });

            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buffer)) => {
                    let input = buffer.trim();
                    if input.is_empty() {
                        continue;
                    }

                    if input.starts_with('/') {
                        let mut command_context = crate::repl::commands::SlashCommandContext {
                            config: &mut self.config,
                            auth_store: &mut self.auth_store,
                            renderer: &self.renderer,
                            session_id: Some(&engine.session_manager.session_id),
                            session_manager: Some(&engine.session_manager),
                        };
                        let result = SlashCommandHandler::handle(input, &mut command_context).await?;
                        if let Some(cmd_res) = result {
                            match cmd_res {
                                CommandResult::Exit => break,
                                CommandResult::OpenModelSelector | CommandResult::OpenSettingsSelector => {
                                    // In legacy reedline mode, fallback to command prompt
                                    continue;
                                }
                                CommandResult::ClearContext => {
                                    engine = crate::platform::agent_engine(
                                        self.config.clone(),
                                        self.auth_store.clone(),
                                        None,
                                    )
                                    .await?;
                                    continue;
                                }
                                CommandResult::ModelChanged {
                                    new_model,
                                    new_provider,
                                } => {
                                    self.config.model = new_model;
                                    if let Some(provider) = new_provider {
                                        self.config.provider = provider;
                                    }
                                    engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                    continue;
                                }
                                CommandResult::Login { provider } => {
                                    crate::cli::login_provider(provider.as_deref(), &self.config, &mut self.auth_store)
                                        .await?;
                                    engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                    continue;
                                }
                                CommandResult::Reload => {
                                    engine = self.reload_engine(&engine).await?;
                                    continue;
                                }
                                CommandResult::Compact { .. } => {
                                    let session_id = engine.session_manager.session_id.clone();
                                    self.renderer.print_notice("  [Compacting conversation context...]\n");
                                    let memory = crate::session::context::context_memory(
                                        engine.session_manager.clone(),
                                        1,
                                        self.config.compaction_max_bytes,
                                    );
                                    let _ = memory.load(&session_id).await;
                                    self.renderer.print_notice("  [Context compaction completed]\n");
                                    continue;
                                }
                                CommandResult::Tree | CommandResult::OpenTreeSelector => {
                                    let tree = engine.session_manager.load_tree().await?;
                                    let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
                                    self.renderer.print_notice(&format!(
                                        "\nConversation Tree (Session: {}):\n{rendered}\n",
                                        engine.session_manager.session_id
                                    ));
                                    continue;
                                }
                                CommandResult::SwitchBranch { leaf_id } => {
                                    let old_leaf = engine.session_manager.active_leaf_id().await?.unwrap_or_default();
                                    let tree = engine.session_manager.load_tree().await?;
                                    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
                                    let has_assistant = abandoned
                                        .iter()
                                        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
                                    if has_assistant
                                        && self.renderer.has_interactive_ui()
                                        && let Ok(true) = inquire::Confirm::new(
                                            "Summarize discoveries from abandoned branch before switching?",
                                        )
                                        .with_default(true)
                                        .prompt()
                                    {
                                        let summary_text = abandoned
                                            .iter()
                                            .map(|n| format!("{:?}", n.messages))
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        let _ = engine
                                            .session_manager
                                            .append_branch_summary(&summary_text, &old_leaf)
                                            .await;
                                    }
                                    let _ = engine.session_manager.switch_branch(Some(leaf_id.clone())).await?;
                                    engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                    self.renderer
                                        .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
                                    continue;
                                }
                                CommandResult::ForkSession { turn_or_node_id } => {
                                    let forked = engine
                                        .session_manager
                                        .fork_session(&self.config.sessions_dir, turn_or_node_id.as_deref())
                                        .await?;
                                    self.renderer
                                        .print_notice(&format!("  [Forked session into {}]\n", forked.session_id));
                                    continue;
                                }
                                CommandResult::CloneSession => {
                                    let cloned =
                                        engine.session_manager.clone_session(&self.config.sessions_dir).await?;
                                    self.renderer
                                        .print_notice(&format!("  [Cloned session into {}]\n", cloned.session_id));
                                    continue;
                                }
                                CommandResult::OpenSessionSelector => {
                                    let summaries =
                                        rho_harness_core::session::list_session_summaries(&self.config.sessions_dir)?;
                                    for s in summaries {
                                        self.renderer.print_notice(&format!(
                                            "  - {} ({}): {}\n",
                                            s.session_id, s.turn_count, s.preview
                                        ));
                                    }
                                    continue;
                                }
                                CommandResult::ResumeSession { session_id } => {
                                    engine = crate::platform::agent_engine(
                                        self.config.clone(),
                                        self.auth_store.clone(),
                                        Some(&session_id),
                                    )
                                    .await?;
                                    self.renderer
                                        .print_notice(&format!("  [Resumed session {session_id}]\n"));
                                    continue;
                                }
                                CommandResult::NameSession { name } => {
                                    engine.session_manager.set_session_name(&name).await?;
                                    self.renderer.print_notice(&format!("  [Named session: \"{name}\"]\n"));
                                    continue;
                                }
                                CommandResult::ExpandedPrompt { text } => {
                                    self.renderer.print_notice("  [Expanded template]\n");
                                    self.renderer.print_user_block(&text);
                                    self.renderer.write_output("\n");
                                    self.run_agent_turn(&engine, crate::engine::runner::TurnRequest::new(&text))
                                        .await?;
                                    engine.refresh_quota().await;
                                    continue;
                                }
                                CommandResult::Rewind { turn } => {
                                    let count = engine.session_manager.rewind_to_turn(turn).await?;
                                    self.renderer.print_notice(&format!(
                                        "  [Rewound context to Turn {turn} ({count} messages retained)]\n"
                                    ));
                                    continue;
                                }
                                CommandResult::Logout { provider } => {
                                    crate::cli::logout_provider(
                                        provider.as_deref(),
                                        &self.config,
                                        &mut self.auth_store,
                                    )?;
                                    engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                                    continue;
                                }
                                CommandResult::Continue => continue,
                            }
                        }
                    }

                    if let Some(cmd) = input.strip_prefix("!!") {
                        let cmd = cmd.trim();
                        if !cmd.is_empty() {
                            self.renderer
                                .print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
                            #[cfg(unix)]
                            let out = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await;
                            #[cfg(windows)]
                            let out = tokio::process::Command::new("cmd.exe")
                                .arg("/c")
                                .arg(cmd)
                                .output()
                                .await;
                            match out {
                                Ok(res) => {
                                    let stdout = String::from_utf8_lossy(&res.stdout);
                                    let stderr = String::from_utf8_lossy(&res.stderr);
                                    if !stdout.is_empty() {
                                        self.renderer.write_output(&stdout);
                                    }
                                    if !stderr.is_empty() {
                                        self.renderer.write_output(&stderr);
                                    }
                                }
                                Err(e) => {
                                    self.renderer
                                        .print_notice(&format!("  Command execution failed: {e}\n"));
                                }
                            }
                            continue;
                        }
                    }

                    let effective_input = if let Some(cmd) = input.strip_prefix('!') {
                        let cmd = cmd.trim();
                        if !cmd.is_empty() {
                            self.renderer
                                .print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
                            #[cfg(unix)]
                            let out = tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await;
                            #[cfg(windows)]
                            let out = tokio::process::Command::new("cmd.exe")
                                .arg("/c")
                                .arg(cmd)
                                .output()
                                .await;
                            match out {
                                Ok(res) => {
                                    let stdout = String::from_utf8_lossy(&res.stdout);
                                    let stderr = String::from_utf8_lossy(&res.stderr);
                                    if !stdout.is_empty() {
                                        self.renderer.write_output(&stdout);
                                    }
                                    if !stderr.is_empty() {
                                        self.renderer.write_output(&stderr);
                                    }
                                    format!(
                                        "Executed local shell command: `{cmd}`\n\nOutput:\n```\n{}{}\n```",
                                        stdout, stderr
                                    )
                                }
                                Err(e) => {
                                    self.renderer
                                        .print_notice(&format!("  Command execution failed: {e}\n"));
                                    format!("Failed to execute local shell command `{cmd}`: {e}")
                                }
                            }
                        } else {
                            input.to_string()
                        }
                    } else {
                        input.to_string()
                    };

                    if stdin_is_tty {
                        clear_submitted_input(input);
                    }
                    self.renderer.print_user_block(&effective_input);
                    self.renderer.write_output("\n");
                    self.run_agent_turn(&engine, crate::engine::runner::TurnRequest::new(&effective_input))
                        .await?;
                    engine.refresh_quota().await;
                }
                Ok(Signal::CtrlC) => {
                    self.renderer.write_output("\nCanceled input.\n");
                }
                Ok(Signal::CtrlD) => {
                    self.renderer.write_output("\nBye.\n");
                    break;
                }
                Err(err) => {
                    self.renderer.write_output(&format!("Input error: {err}\n"));
                    break;
                }
            }
        }

        Ok(())
    }

    async fn run_agent_turn(
        &self,
        engine: &AgentEngine,
        request: crate::engine::runner::TurnRequest<'_>,
    ) -> Result<()> {
        let renderer = &self.renderer;
        let run_future = engine.run_turn(request, std::sync::Arc::new(renderer.clone()));
        tokio::select! {
            run_res = run_future => {
                renderer.flush();
                renderer.write_output("\n");
                if let Err(error) = run_res {
                    renderer.write_output(&format!("\nError: {error}\n"));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                renderer.flush();
                engine.record_cancellation("operator interrupt").await?;
                renderer.write_output("\nCanceled.\n");
            }
        }
        Ok(())
    }
}
