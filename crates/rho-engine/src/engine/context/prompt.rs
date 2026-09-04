use super::ProjectContext;

pub fn build_system_prompt(ctx: &ProjectContext) -> String {
    let mut prompt = String::new();
    prompt.push_str(ctx.base_system_prompt.trim());
    prompt.push_str("\n\n");

    if !ctx.instruction_files.is_empty() {
        prompt.push_str("<project_context>\n\nProject-specific instructions and guidelines:\n\n");
        for (name, content) in &ctx.instruction_files {
            prompt.push_str(&format!(
                "<project_instructions path=\"{}\">\n{}\n</project_instructions>\n\n",
                escape_xml(name),
                content
            ));
        }
        prompt.push_str("</project_context>\n\n");
    }

    if !ctx.skills.is_empty() {
        prompt.push_str("The following skills provide specialized instructions for specific tasks.\n");
        prompt.push_str("Use the read tool to load a skill's file when the task matches its description.\n");
        prompt.push_str("When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n");
        prompt.push_str("<available_skills>\n");
        for skill in &ctx.skills {
            prompt.push_str("  <skill>\n");
            prompt.push_str(&format!("    <name>{}</name>\n", escape_xml(&skill.name)));
            prompt.push_str(&format!(
                "    <description>{}</description>\n",
                escape_xml(&skill.description)
            ));
            prompt.push_str(&format!("    <location>{}</location>\n", escape_xml(&skill.location)));
            prompt.push_str("  </skill>\n");
        }
        prompt.push_str("</available_skills>\n\n");
    }

    let clean_cwd = ctx.current_dir.display().to_string().replace('\\', "/");
    prompt.push_str(&format!("Current working directory: {clean_cwd}\n\n"));
    prompt.push_str(&format!(
        "Today's date is {}. When searching for recent events, releases, or \"latest\" information, factor in this current date.\n",
        ctx.date_str
    ));
    prompt.push_str(&format!("Platform: {}", ctx.os_info));

    if let Some(ref git) = ctx.git_status {
        prompt.push_str(&format!("\nGit repository status: {git}"));
    }

    prompt
}

pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
