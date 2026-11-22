pub fn build_guidelines(active_tools: &[String]) -> Vec<&'static str> {
    let mut guidelines = Vec::new();
    append_search_guidelines(active_tools, &mut guidelines);
    append_tool_guidelines(active_tools, &mut guidelines);
    append_universal_guidelines(&mut guidelines);
    guidelines
}

fn has_tool(active_tools: &[String], name: &str) -> bool {
    active_tools.iter().any(|t| t == name)
}

fn append_search_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    let has_fd = has_tool(active_tools, "fd");
    let has_rg = has_tool(active_tools, "rg");
    match (has_fd, has_rg) {
        (true, true) => out.push("Use fd to find files and rg to search file contents instead of find, ls, or grep"),
        (true, false) => out.push("Use fd to find files instead of find or ls"),
        (false, true) => out.push("Use rg to search file contents instead of grep"),
        (false, false) if has_tool(active_tools, "bash") => {
            out.push("Use bash for file operations like ls, rg, find");
        }
        _ => {}
    }
}

fn append_tool_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    if has_tool(active_tools, "bash") {
        let has_specialized = active_tools.iter().any(|t| {
            t == "tool_search"
                || t.starts_with("mcp__")
                || (!matches!(
                    t.as_str(),
                    "bash"
                        | "read"
                        | "write"
                        | "edit"
                        | "undo"
                        | "fd"
                        | "rg"
                        | "web_search"
                        | "web_fetch"
                        | "subagent"
                        | "worktree"
                        | "lsp"
                        | "memory"
                        | "todo"
                        | "wait"
                ))
        });
        if has_specialized {
            out.push("Never use bash when a specialized tool or MCP tool exists; prefer dedicated tools, find unlisted MCP tools with tool_search, and call them through the mcp tool");
        }
    }
    if has_tool(active_tools, "read") {
        out.push("Use read to examine files instead of cat or sed");
    }
    if has_tool(active_tools, "lsp") {
        out.push("Use lsp for definitions, references, and diagnostics; use rg for plain text search");
    }
    append_mutation_guidelines(active_tools, out);
}

fn append_mutation_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    let has_edit = has_tool(active_tools, "edit");
    let has_write = has_tool(active_tools, "write");
    if has_edit {
        out.push("Use edit for every change to an existing file");
    }
    if has_write {
        out.push("Use write only for new files or complete rewrites");
    }
    if (has_edit || has_write) && has_tool(active_tools, "bash") {
        out.push("Never create or modify files through bash (sed -i, perl -pi, python or awk scripts, echo or cat redirects, heredocs, tee, mv over a file); project formatters and generators such as cargo fmt are fine");
    }
}

fn append_universal_guidelines(out: &mut Vec<&'static str>) {
    out.push("Work autonomously until the request is fully resolved; do not stop to ask for input you can discover or act on with your tools");
    out.push("Verify changes before finishing (tests, builds, linters, or command output); never claim success when checks fail");
    out.push("Batch independent read-only calls in one response so they run in parallel");
    out.push("Be concise in your responses and show file paths clearly");
    out.push("When the user or project instructions conflict with these guidelines, follow the user and project instructions");
}
