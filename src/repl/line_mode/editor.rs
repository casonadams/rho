use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::completer::RhoCompleter;
use crate::ui::render::WelcomeDisplay;
use reedline::{
    ColumnarMenu, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu,
    default_emacs_keybindings,
};

pub fn build_line_editor(config: &Config, auth_store: &AuthStore) -> Result<Reedline> {
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::ALT,
        KeyCode::Enter,
        ReedlineEvent::Edit(vec![reedline::EditCommand::InsertNewline]),
    );
    let edit_mode = Box::new(Emacs::new(keybindings));

    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let prompt_templates = rho_harness_core::prompts::discover_prompt_templates(
        Some(&config.config_dir),
        std::env::current_dir().ok().as_deref(),
    )
    .into_iter()
    .map(|t| t.metadata.name)
    .collect::<Vec<_>>();
    let models = crate::repl::interactive::discover_models(config, auth_store);
    let custom_providers = config.providers.keys().cloned().collect();
    let sources = crate::repl::interactive::CompletionSources::new()
        .with_skills(skills)
        .with_templates(prompt_templates)
        .with_models(models)
        .with_custom_providers(custom_providers);
    let completer = Box::new(RhoCompleter::new(sources));
    let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));

    let history = Box::new(
        FileBackedHistory::with_file(1000, config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?,
    );

    Ok(Reedline::create()
        .with_history(history)
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode))
}

pub fn print_line_mode_welcome(session: &ReplSession, engine: &AgentEngine) {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let tools = engine.tool_names();
    let mut plugins = session.config.plugins.keys().cloned().collect::<Vec<_>>();
    for mcp in session.config.mcp.servers.keys() {
        if !plugins.contains(mcp) {
            plugins.push(mcp.clone());
        }
    }

    session.renderer.print_welcome(&WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        auto_approve: session.config.auto_approve,
        resumed: session.resume_id.is_some(),
        tools,
        skills: skill_names,
        plugins,
    });
}
