use crate::permission::baseline::{is_baseline_bash, is_baseline_tool};

#[test]
fn baseline_rules_match_inspection_commands() {
    assert!(is_baseline_tool("read"));
    assert!(is_baseline_tool("write"));
    assert!(is_baseline_tool("edit"));
    assert!(is_baseline_tool("grep"));
    assert!(is_baseline_tool("find"));
    assert!(is_baseline_tool("ls"));
    assert!(is_baseline_tool("fetch"));
    assert!(is_baseline_tool("search"));
    assert!(is_baseline_tool("fd"));
    assert!(is_baseline_tool("rg"));
    assert!(!is_baseline_tool("unknown_tool"));

    assert!(is_baseline_bash("git status"));
    assert!(is_baseline_bash("git diff HEAD"));
    assert!(is_baseline_bash("git log -n 5"));
    assert!(is_baseline_bash("pwd"));
    assert!(is_baseline_bash("ls -la"));
    assert!(is_baseline_bash("rg foo src/"));
    assert!(is_baseline_bash("cat Cargo.toml"));
    assert!(is_baseline_bash("jq . package.json"));
    assert!(is_baseline_bash("uname -a"));
    assert!(is_baseline_bash("node --version"));
    assert!(is_baseline_bash("cargo -v"));
    assert!(is_baseline_bash("python --help"));
    assert!(!is_baseline_bash("rm -rf /"));
    assert!(!is_baseline_bash("git push origin main"));
}
