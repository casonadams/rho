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
        | "find" | "wc" | "diff" | "file" | "stat" | "uname" | "printenv" | "true" | "false" => true,
        "git" => {
            if let Some(sub) = tokens.get(1) {
                match *sub {
                    "status" | "diff" | "log" | "show" | "describe" => true,
                    "branch" => tokens
                        .iter()
                        .any(|&t| t == "--show-current" || t == "-a" || t == "-r" || t == "--list" || t == "-l"),
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
                )
            } else {
                false
            }
        }
        _ => false,
    }
}
