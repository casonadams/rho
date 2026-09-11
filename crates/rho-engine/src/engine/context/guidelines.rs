pub fn build_guidelines(active_tools: &[String]) -> Vec<&'static str> {
    let mut guidelines = Vec::new();
    append_search_guidelines(active_tools, &mut guidelines);
    append_tool_guidelines(active_tools, &mut guidelines);
    append_universal_guidelines(&mut guidelines);
    guidelines
}

fn append_search_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    let has_fd = active_tools.iter().any(|t| t == "fd");
    let has_rg = active_tools.iter().any(|t| t == "rg");
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
        (false, false) if active_tools.iter().any(|t| t == "bash") => {
            out.push("Use bash for file operations like ls, rg, find");
        }
        _ => {}
    }
}

fn append_tool_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    if active_tools.iter().any(|t| t == "bash") {
        out.push("Commands run directly in the working directory; do not prefix commands with cd");
        let has_inspectors = active_tools.iter().any(|t| matches!(t.as_str(), "fd" | "rg" | "read"));
        if has_inspectors {
            out.push("Never use bash for file inspection (ls, find, grep, cat); use fd, rg, and read instead");
        }
    }
    if active_tools.iter().any(|t| t == "read") {
        out.push("Use read to examine files instead of cat or sed");
    }
    append_mutation_guidelines(active_tools, out);
}

fn append_mutation_guidelines(active_tools: &[String], out: &mut Vec<&'static str>) {
    if active_tools.iter().any(|t| t == "edit") {
        out.push("Use edit for precise changes (edits[].oldText must match exactly)");
        out.push("When changing multiple separate locations in one file, use one edit call with multiple entries in edits[] instead of multiple edit calls");
        out.push("Keep edits[].oldText as small as possible while still being unique in the file");
    }
    if active_tools.iter().any(|t| t == "write") {
        out.push("Use write only for new files or complete rewrites");
    }
}

fn append_universal_guidelines(out: &mut Vec<&'static str>) {
    out.push("Be concise in your responses");
    out.push("Show file paths clearly when working with files");
}
