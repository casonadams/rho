/// Classifies whether a shell command is read-only.
pub fn is_read_only_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.contains('>') || cmd.contains("$(") || cmd.contains('`') {
        return false;
    }
    let subcommands: Vec<&str> = cmd
        .split([';', '&', '|'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    subcommands.iter().all(|sub| is_single_read_only_command(sub))
}

const READ_ONLY_BINARIES: &[&str] = &[
    "ls", "pwd", "whoami", "which", "whereis", "echo", "printf", "cat", "head", "tail", "grep", "rg", "find", "wc",
    "diff", "file", "stat", "uname", "printenv", "true", "false", "sort", "uniq", "cut", "tr", "cmp", "comm", "column",
    "jq", "date", "uptime", "id", "arch", "hostname", "locale", "type",
];

fn is_pure_read_only_binary(exe: &str) -> bool {
    READ_ONLY_BINARIES.contains(&exe)
}

fn is_runtime_version_command(exe: &str, tokens: &[&str]) -> bool {
    match exe {
        "python" | "python3" | "rustc" => tokens.iter().any(|&t| t == "--version" || t == "-V"),
        "node" => tokens.iter().any(|&t| t == "--version" || t == "-v"),
        _ => false,
    }
}

fn is_npm_or_yarn_read_only(exe: &str, sub: Option<&str>, tokens: &[&str]) -> bool {
    let has_version = tokens.iter().any(|&t| t == "--version" || t == "-v");
    match sub {
        Some(s) if exe == "yarn" => matches!(s, "list" | "test" | "why") || has_version,
        Some(s) => matches!(s, "list" | "ls" | "test" | "view" | "info" | "outdated") || has_version,
        None => has_version,
    }
}

fn is_package_manager_read_only(exe: &str, tokens: &[&str]) -> bool {
    let sub = tokens.get(1).copied();
    match exe {
        "go" => sub.is_some_and(|s| matches!(s, "version" | "list" | "test")),
        "npm" | "pnpm" | "yarn" => is_npm_or_yarn_read_only(exe, sub, tokens),
        "cargo" => match sub {
            Some(s) => {
                matches!(
                    s,
                    "check" | "clippy" | "test" | "fmt" | "tree" | "metadata" | "verify-project" | "read-manifest"
                ) || tokens.iter().any(|&t| t == "--version" || t == "-V")
            }
            None => tokens.iter().any(|&t| t == "--version" || t == "-V"),
        },
        _ => false,
    }
}

fn is_git_subcommand_read_only(sub: &str, tokens: &[&str]) -> bool {
    match sub {
        "status" | "diff" | "log" | "show" | "describe" | "rev-parse" => true,
        "branch" => tokens
            .iter()
            .any(|&t| matches!(t, "--show-current" | "-a" | "-r" | "--list" | "-l")),
        "tag" => tokens.len() == 2 || tokens.iter().any(|&t| matches!(t, "-l" | "--list")),
        "remote" => tokens
            .iter()
            .all(|&t| !matches!(t, "add" | "remove" | "rm" | "set-url")),
        "config" => tokens.iter().any(|&t| matches!(t, "--get" | "--list" | "-l")),
        _ => false,
    }
}

fn is_git_read_only(tokens: &[&str]) -> bool {
    match tokens.get(1) {
        Some(sub) => is_git_subcommand_read_only(sub, tokens),
        None => true,
    }
}

fn is_single_read_only_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains("-delete") || lower.contains("-exec") {
        return false;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(first) = tokens.first() else {
        return true;
    };
    let exe = first.split('/').next_back().unwrap_or(first).to_ascii_lowercase();
    is_pure_read_only_binary(&exe)
        || is_runtime_version_command(&exe, &tokens)
        || is_package_manager_read_only(&exe, &tokens)
        || (exe == "git" && is_git_read_only(&tokens))
}
