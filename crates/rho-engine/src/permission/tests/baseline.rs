use crate::permission::baseline::{is_baseline_bash, is_baseline_tool};

#[test]
fn baseline_tools_allowed() {
    let tools = [
        "read", "write", "edit", "grep", "find", "ls", "fetch", "search", "fd", "rg",
    ];
    for t in tools {
        assert!(is_baseline_tool(t));
    }
    assert!(!is_baseline_tool("unknown_tool"));
}

#[test]
fn baseline_bash_allowed() {
    let bash_cmds = [
        "git status",
        "git diff HEAD",
        "git log -n 5",
        "pwd",
        "ls -la",
        "rg foo src/",
        "cat Cargo.toml",
        "jq . package.json",
        "uname -a",
        "node --version",
        "cargo -v",
        "python --help",
    ];
    for cmd in bash_cmds {
        assert!(is_baseline_bash(cmd));
    }
    assert!(!is_baseline_bash("rm -rf /") && !is_baseline_bash("git push origin main"));
}
