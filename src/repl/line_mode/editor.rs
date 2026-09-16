use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::ui::render::WelcomeDisplay;

pub async fn print_line_mode_welcome(session: &ReplSession, engine: &AgentEngine) {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let tools = engine.tool_names();
    let mcp = session.config.mcp.servers.keys().cloned().collect::<Vec<_>>();
    let agents = engine.instruction_files().await;

    session.renderer.print_welcome(&WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        mcp,
    });
}
