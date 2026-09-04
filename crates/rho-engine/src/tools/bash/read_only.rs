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

    match exe.as_str() {
        "ls" | "pwd" | "whoami" | "which" | "whereis" | "echo" | "printf" | "cat" | "head" | "tail" | "grep" | "rg"
        | "find" | "wc" | "diff" | "file" | "stat" | "uname" | "printenv" | "true" | "false" | "sort" | "uniq"
        | "cut" | "tr" | "cmp" | "comm" | "column" | "jq" | "date" | "uptime" | "id" | "arch" | "hostname"
        | "locale" | "type" => true,
        "python" | "python3" => tokens.iter().any(|&t| t == "--version" || t == "-V"),
        "node" => tokens.iter().any(|&t| t == "--version" || t == "-v"),
        "rustc" => tokens.iter().any(|&t| t == "--version" || t == "-V"),
        "go" => {
            if let Some(sub) = tokens.get(1) {
                matches!(*sub, "version" | "list" | "test")
            } else {
                false
            }
        }
        "npm" | "pnpm" => {
            if let Some(sub) = tokens.get(1) {
                matches!(*sub, "list" | "ls" | "test" | "view" | "info" | "outdated")
                    || tokens.iter().any(|&t| t == "--version" || t == "-v")
            } else {
                tokens.iter().any(|&t| t == "--version" || t == "-v")
            }
        }
        "yarn" => {
            if let Some(sub) = tokens.get(1) {
                matches!(*sub, "list" | "test" | "why") || tokens.iter().any(|&t| t == "--version" || t == "-v")
            } else {
                tokens.iter().any(|&t| t == "--version" || t == "-v")
            }
        }
        "git" => {
            if let Some(sub) = tokens.get(1) {
                match *sub {
                    "status" | "diff" | "log" | "show" | "describe" | "rev-parse" => true,
                    "branch" => tokens
                        .iter()
                        .any(|&t| t == "--show-current" || t == "-a" || t == "-r" || t == "--list" || t == "-l"),
                    "tag" => tokens.iter().any(|&t| t == "-l" || t == "--list") || tokens.len() == 2,
                    "remote" => tokens
                        .iter()
                        .all(|&t| t != "add" && t != "remove" && t != "rm" && t != "set-url"),
                    "config" => tokens.iter().any(|&t| t == "--get" || t == "--list" || t == "-l"),
                    _ => false,
                }
            } else {
                true
            }
        }
        "cargo" => {
            if let Some(sub) = tokens.get(1) {
                matches!(
                    *sub,
                    "check" | "clippy" | "test" | "fmt" | "tree" | "metadata" | "verify-project" | "read-manifest"
                ) || tokens.iter().any(|&t| t == "--version" || t == "-V")
            } else {
                tokens.iter().any(|&t| t == "--version" || t == "-V")
            }
        }
        _ => false,
    }
}
