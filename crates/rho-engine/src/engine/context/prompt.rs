use super::ProjectContext;
use super::guidelines::build_guidelines;
use rho_harness_core::args::MemoryScope;
use rho_harness_core::memory::{MemoryStore, render_notes};

fn format_tool_line(tool_name: &str) -> String {
    match crate::tools::ToolRegistry::prompt_snippet(tool_name) {
        Some(snippet) => format!("- {tool_name}: {snippet}\n"),
        None => format!("- {tool_name}\n"),
    }
}

fn append_available_tools(prompt: &mut String, active_tools: &[String]) {
    prompt.push_str("Available tools:\n");
    if active_tools.is_empty() {
        prompt.push_str("(none)\n");
        return;
    }
    for tool_name in active_tools {
        prompt.push_str(&format_tool_line(tool_name));
    }
}

pub fn assemble_base_system_prompt(active_tools: &[String]) -> String {
    let mut prompt = String::with_capacity(1024);
    prompt.push_str(
        "You are an expert coding assistant operating inside rho, a coding agent harness. \
         You help users by reading files, executing commands, editing code, and writing new files.\n\n",
    );
    append_available_tools(&mut prompt, active_tools);
    prompt.push_str(
        "\nIn addition to the tools above, you may have access to other custom tools depending on the project.\n\nGuidelines:\n",
    );
    for guideline in build_guidelines(active_tools) {
        prompt.push_str(&format!("- {guideline}\n"));
    }
    prompt
}

fn append_instruction_files(prompt: &mut String, files: &[(String, String)]) {
    if files.is_empty() {
        return;
    }
    prompt.push_str("<project_context>\n\nProject-specific instructions and guidelines:\n\n");
    for (name, content) in files {
        prompt.push_str(&format!(
            "<project_instructions path=\"{}\">\n{}\n</project_instructions>\n\n",
            escape_xml(name),
            content
        ));
    }
    prompt.push_str("</project_context>\n\n");
}

/// The saved memory notes as a `<memory>` block, or empty when there are none.
pub(super) fn memory_section(store: &MemoryStore) -> String {
    let mut body = String::new();
    for (scope, label) in [(MemoryScope::Project, "Project"), (MemoryScope::Global, "Global")] {
        match store.loaded_notes(scope) {
            Ok(loaded) if loaded.notes.is_empty() => {}
            Ok(loaded) => {
                body.push_str(&format!("{label} notes:\n{}", escape_xml(&render_notes(&loaded.notes))));
                if loaded.omitted > 0 {
                    body.push_str(&format!(
                        "[{} more {label} notes omitted: the file is over the memory limit]\n",
                        loaded.omitted
                    ));
                }
            }
            Err(error) => body.push_str(&format!("{label} notes could not be read: {error}\n")),
        }
    }
    if body.is_empty() {
        return body;
    }
    format!("<memory>\nNotes saved with the memory tool in earlier sessions:\n\n{body}</memory>\n\n")
}

fn skill_reader_tool(active_tools: &[String]) -> Option<&'static str> {
    if active_tools.iter().any(|t| t == "read") {
        Some("the read tool")
    } else if active_tools.iter().any(|t| t == "bash") {
        Some("bash")
    } else {
        None
    }
}

fn format_skill_entry(prompt: &mut String, skill: &rho_harness_core::skills::SkillMetadata) {
    prompt.push_str("  <skill>\n");
    prompt.push_str(&format!("    <name>{}</name>\n", escape_xml(&skill.name)));
    prompt.push_str(&format!(
        "    <description>{}</description>\n",
        escape_xml(&skill.description)
    ));
    prompt.push_str(&format!("    <location>{}</location>\n", escape_xml(&skill.location)));
    prompt.push_str("  </skill>\n");
}

fn append_skills(prompt: &mut String, skills: &[rho_harness_core::skills::SkillMetadata], active_tools: &[String]) {
    let visible: Vec<_> = skills.iter().filter(|s| !s.disable_model_invocation).collect();
    let Some(reader) = skill_reader_tool(active_tools).filter(|_| !visible.is_empty()) else {
        return;
    };
    prompt.push_str("The following skills provide specialized instructions for specific tasks.\n");
    prompt.push_str(&format!(
        "Use {reader} to load a skill's file when the task matches its description; read the entire skill file on first read without a line limit.\n"
    ));
    prompt.push_str("When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n");
    prompt.push_str("<available_skills>\n");
    for skill in visible {
        format_skill_entry(prompt, skill);
    }
    prompt.push_str("</available_skills>\n\n");
}

fn append_agents(prompt: &mut String, agents: &[rho_harness_core::agents::AgentMetadata], active_tools: &[String]) {
    if !active_tools.iter().any(|t| t == "subagent") || agents.is_empty() {
        return;
    }
    prompt.push_str("The following custom subagents are available for delegated tasks via the subagent tool:\n");
    prompt.push_str("<available_agents>\n");
    for agent in agents {
        prompt.push_str("  <agent>\n");
        prompt.push_str(&format!("    <name>{}</name>\n", escape_xml(&agent.name)));
        prompt.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&agent.description)
        ));
        prompt.push_str("  </agent>\n");
    }
    prompt.push_str("</available_agents>\n\n");
}

fn append_environment_info(prompt: &mut String, ctx: &ProjectContext) {
    let clean_cwd = ctx.current_dir.display().to_string().replace('\\', "/");
    prompt.push_str(&format!("Current working directory: {clean_cwd}\n\n"));
    prompt.push_str(&format!(
        "Today's date is {}. When searching for recent events, releases, or \"latest\" information, factor in this current date.\n",
        ctx.date_str
    ));
    prompt.push_str(&format!("Platform: {}", ctx.os_info));
}

pub fn format_turn_prompt(prompt: &str, git_status: Option<&str>) -> String {
    if let Some(git) = git_status.filter(|s| !s.trim().is_empty()) {
        format!("{}{git}]\n\n{prompt}", rho_harness_core::prompts::GIT_STATUS_PREFIX)
    } else {
        prompt.to_string()
    }
}

pub fn build_system_prompt(ctx: &ProjectContext) -> String {
    let mut prompt = format!("{}\n\n", ctx.base_system_prompt.trim());
    append_instruction_files(&mut prompt, &ctx.instruction_files);
    prompt.push_str(&ctx.memory);
    append_skills(&mut prompt, &ctx.skills, &ctx.active_tools);
    append_agents(&mut prompt, &ctx.agents, &ctx.active_tools);
    append_environment_info(&mut prompt, ctx);
    prompt
}

pub fn build_agent_grounding(ctx: &ProjectContext, agent_tools: &[String]) -> String {
    let mut prompt = String::with_capacity(1024);
    append_available_tools(&mut prompt, agent_tools);
    let guidelines = build_guidelines(agent_tools);
    if !guidelines.is_empty() {
        prompt.push_str("\nGuidelines:\n");
        for guideline in guidelines {
            prompt.push_str(&format!("- {guideline}\n"));
        }
    }
    prompt.push('\n');
    append_instruction_files(&mut prompt, &ctx.instruction_files);
    prompt.push_str(&ctx.memory);
    append_environment_info(&mut prompt, ctx);
    if let Some(git) = ctx.git_status.as_deref().filter(|s| !s.trim().is_empty()) {
        prompt.push_str(&format!("\nGit repository status: {git}"));
    }
    prompt
}

pub fn escape_xml(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.bytes().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'')) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    std::borrow::Cow::Owned(out)
}
