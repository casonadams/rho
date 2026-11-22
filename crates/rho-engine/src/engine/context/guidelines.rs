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
        (true, true) => {
            out.push(
                "Use fd for file discovery and directory structure instead of find, glob, or ls; use depth for bounded overviews",
            );
            out.push("Use rg for content search instead of grep or bash pipelines; narrow with path or type");
        }
        (true, false) => {
            out.push("Use fd for file discovery and directory structure instead of find, glob, or ls; use depth for bounded overviews");
        }
        (false, true) => {
            out.push("Use rg for content search instead of grep or bash pipelines; narrow with path or type")
        }
        (false, false) if has_tool(active_tools, "bash") => {
            out.push("Use bash for file operations like ls, rg, find");
        }
        _ => {}
    }
}

fn append_tool_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    if has_tool(active_tools, "bash") {
        out.push("Commands run directly in the working directory; do not prefix commands with cd");
        let has_inspectors = active_tools.iter().any(|t| matches!(t.as_str(), "fd" | "rg" | "read"));
        if has_inspectors {
            out.push("Never use bash for file inspection (ls, find, grep, cat); use fd, rg, and read instead");
        }
        let has_specialized = active_tools.iter().any(|t| {
            t == "tool_search"
                || t.starts_with("mcp__")
                || (!matches!(
                    t.as_str(),
                    "bash"
                        | "read"
                        | "write"
                        | "edit"
                        | "fd"
                        | "rg"
                        | "web_search"
                        | "web_fetch"
                        | "subagent"
                        | "worktree"
                        | "lsp"
                        | "memory"
                        | "todo"
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
        out.push("Use lsp for definitions, references, hover, and diagnostics in files a language server handles; use rg for plain text search");
    }
    append_mutation_guidelines(active_tools, out);
    if has_tool(active_tools, "bash") {
        out.push(
            "Background jobs: Results auto-deliver upon completion. Never poll or call sleep while waiting; continue other productive work or end your turn to wait for the completion signal",
        );
    }
}

fn append_mutation_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    let has_edit = has_tool(active_tools, "edit");
    let has_write = has_tool(active_tools, "write");
    if has_edit {
        out.push("Use edit for every change to an existing file: oldText for small unique changes, replaceAll for renames within a file, and startText/endText to delete or rewrite a whole block");
        out.push("Put all changes to one file in one edit call; for changes across files, issue one edit call per file in the same response");
        out.push("Keep edits[].oldText as small as possible while still being unique in the file");
    }
    if has_write {
        out.push("Use write only for new files or complete rewrites");
    }
    if (has_edit || has_write) && has_tool(active_tools, "bash") {
        out.push("Never create or modify files through bash (sed -i, perl -pi, python or awk scripts, echo/cat redirects, heredocs, tee, mv over a file); use edit and write. Project formatters and generators such as cargo fmt are fine");
    }
}

fn append_universal_guidelines(out: &mut Vec<&'static str>) {
    out.push("Drive tasks autonomously to completion: continue using tools step-by-step to read, edit, execute, and verify until the user request is completely resolved");
    out.push("Do not stop prematurely or ask for user input when you can discover the information or make progress using available tools");
    out.push(
        "Always verify your changes before finishing (run tests, check build diagnostics, or inspect command outputs); never claim success when checks fail",
    );
    out.push("Batch independent read-only calls (read, fd, rg, web_search, web_fetch) in one response so they run in parallel");
    out.push("Be concise in your responses");
    out.push("Show file paths clearly when working with files");
}
