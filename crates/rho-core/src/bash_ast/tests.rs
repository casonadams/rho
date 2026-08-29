//! Behavioural and performance tests for bash command safety analysis.
//!
//! The classification cases are large because the safety analysis is the gate
//! between the agent and `bash` execution — the test corpus intentionally
//! covers read-only commands (which must stay read-only), mutating commands
//! (which must be flagged), high-risk commands (which must be flagged at the
//! top tier), shell structural forms (subshells, case clauses, for loops,
//! arithmetic, heredocs, command substitutions), and parser-error inputs.
//!
//! [`analyze_command_safety`] is the public entry point under test.

use super::{RiskTier, analyze_command_safety};

#[test]
fn risk_tiers_are_ordered() {
    assert!(RiskTier::ReadOnly < RiskTier::Mutating);
    assert!(RiskTier::Mutating < RiskTier::HighRisk);
}

#[test]
fn classifies_shell_structures() {
    let cases = [
        ("ls -la", RiskTier::ReadOnly),
        ("cat Cargo.toml", RiskTier::ReadOnly),
        ("head -n 2 Cargo.toml", RiskTier::ReadOnly),
        ("tail -n 2 Cargo.toml", RiskTier::ReadOnly),
        ("pwd", RiskTier::ReadOnly),
        ("rg 'fn main' src", RiskTier::ReadOnly),
        ("grep -R needle src", RiskTier::ReadOnly),
        ("find . -name '*.rs'", RiskTier::ReadOnly),
        ("git status", RiskTier::ReadOnly),
        ("git diff --stat", RiskTier::ReadOnly),
        ("git log -1", RiskTier::ReadOnly),
        ("git show HEAD:file", RiskTier::ReadOnly),
        ("git rev-parse HEAD", RiskTier::ReadOnly),
        ("git ls-files", RiskTier::ReadOnly),
        ("git describe --tags", RiskTier::ReadOnly),
        ("git branch --show-current", RiskTier::ReadOnly),
        ("git branch -a", RiskTier::ReadOnly),
        ("git tag --list", RiskTier::ReadOnly),
        ("git remote -v", RiskTier::ReadOnly),
        ("git remote get-url origin", RiskTier::ReadOnly),
        ("git config --get user.name", RiskTier::ReadOnly),
        ("cargo check", RiskTier::ReadOnly),
        ("cargo clippy --all-targets", RiskTier::ReadOnly),
        ("cargo test --all-targets", RiskTier::ReadOnly),
        ("cargo tree", RiskTier::ReadOnly),
        ("cargo metadata", RiskTier::ReadOnly),
        ("cat Cargo.toml | rg rig", RiskTier::ReadOnly),
        ("git status && cargo check", RiskTier::ReadOnly),
        ("false || git status", RiskTier::ReadOnly),
        ("(git status && cargo check) | rg error", RiskTier::ReadOnly),
        ("{ git status; cargo check; }", RiskTier::ReadOnly),
        ("cat < Cargo.toml", RiskTier::ReadOnly),
        ("cd .", RiskTier::ReadOnly),
        ("cd ./ && git status", RiskTier::ReadOnly),
        ("git status 2>/dev/null", RiskTier::ReadOnly),
        ("echo test > /dev/stdout", RiskTier::ReadOnly),
        ("git status 2>&1", RiskTier::ReadOnly),
        ("echo \"Commit: $(git rev-parse HEAD)\"", RiskTier::ReadOnly),
        ("printf '%s' `git rev-parse HEAD`", RiskTier::ReadOnly),
        ("diff <(git show HEAD:file) <(cat file)", RiskTier::ReadOnly),
        ("RUST_LOG=debug cargo check", RiskTier::ReadOnly),
        (
            "if git status; then cargo check; else cargo test; fi",
            RiskTier::ReadOnly,
        ),
        ("for f in a b; do echo \"$f\"; done", RiskTier::ReadOnly),
        ("cat <<'EOF'\nhello\nEOF", RiskTier::ReadOnly),
        ("[[ $(git status) == '' ]]", RiskTier::ReadOnly),
        ("echo test > test.txt", RiskTier::Mutating),
        ("cat >> output.log", RiskTier::Mutating),
        ("cat <> state", RiskTier::Mutating),
        ("cat Cargo.toml | tee copy.toml", RiskTier::Mutating),
        ("touch marker", RiskTier::Mutating),
        ("cd ..", RiskTier::Mutating),
        ("cd subdir && cat secrets.txt", RiskTier::Mutating),
        ("git commit -m msg", RiskTier::Mutating),
        ("git checkout main", RiskTier::Mutating),
        ("git branch new", RiskTier::Mutating),
        ("git config user.name model", RiskTier::Mutating),
        ("cargo run", RiskTier::Mutating),
        ("npm install", RiskTier::Mutating),
        ("find . -delete", RiskTier::Mutating),
        ("find . -exec cat {} ';'", RiskTier::Mutating),
        ("eval \"$USER_INPUT\"", RiskTier::Mutating),
        ("exec sh", RiskTier::Mutating),
        ("source script.sh", RiskTier::Mutating),
        ("$CMD arg", RiskTier::Mutating),
        ("echo $(touch marker)", RiskTier::Mutating),
        ("[[ $(touch marker) == x ]]", RiskTier::Mutating),
        ("for ((i=0; i<1; i++)); do rm -rf target; done", RiskTier::HighRisk),
        ("echo $(( $(rm -rf target) + 1 ))", RiskTier::HighRisk),
        ("printf `touch marker`", RiskTier::Mutating),
        ("foo() { touch marker; }; foo", RiskTier::Mutating),
        ("echo test > /etc/hosts", RiskTier::HighRisk),
        ("echo test > /dev/sda", RiskTier::HighRisk),
        ("rm -rf target", RiskTier::HighRisk),
        ("rm -f file", RiskTier::HighRisk),
        ("rm --recursive target", RiskTier::HighRisk),
        ("unlink '*.tmp'", RiskTier::HighRisk),
        ("git reset --hard HEAD~1", RiskTier::HighRisk),
        ("git clean -fd", RiskTier::HighRisk),
        ("git push --force origin main", RiskTier::HighRisk),
        ("git checkout -- .", RiskTier::HighRisk),
        ("git restore .", RiskTier::HighRisk),
        ("sudo apt install x", RiskTier::HighRisk),
        ("doas reboot", RiskTier::HighRisk),
        ("mkfs /dev/sda", RiskTier::HighRisk),
        ("dd if=/dev/zero of=/dev/sda", RiskTier::HighRisk),
        ("shred file", RiskTier::HighRisk),
        ("echo 'unterminated", RiskTier::Mutating),
        ("if true; then echo x", RiskTier::Mutating),
        ("", RiskTier::Mutating),
    ];

    assert!(cases.len() >= 50);
    for (command, expected) in cases {
        let analysis = analyze_command_safety(command);
        assert_eq!(analysis.tier, expected, "{command}: {:?}", analysis.reasons);
        if expected != RiskTier::ReadOnly {
            assert!(!analysis.reasons.is_empty(), "{command}");
        }
    }
}

#[test]
fn derives_session_patterns_from_command_and_first_argument() {
    let analysis =
        analyze_command_safety("cargo test --all-targets && git commit -m update && touch marker && rm -rf target");
    assert_eq!(
        analysis.session_patterns.unwrap(),
        ["cargo test *", "git commit *", "rm -rf *", "touch *"]
    );
}

#[test]
fn dynamic_arguments_disable_session_patterns() {
    let analysis = analyze_command_safety("touch \"$TARGET\"");
    assert_eq!(analysis.session_patterns, None);
}

#[test]
fn reports_nested_commands() {
    let analysis = analyze_command_safety("echo $(git status) | rg clean");
    assert_eq!(analysis.commands, ["git", "echo", "rg"]);
}

#[test]
fn standard_command_analysis_averages_under_one_millisecond() {
    let commands = [
        "git status && cargo check",
        "cat Cargo.toml | rg brush",
        "echo \"Commit: $(git rev-parse HEAD)\"",
        "echo test > output.txt",
    ];
    for command in commands {
        analyze_command_safety(command);
    }

    let iterations = 500;
    let started = std::time::Instant::now();
    for _ in 0..iterations {
        for command in commands {
            std::hint::black_box(analyze_command_safety(command));
        }
    }
    let average = started.elapsed() / (iterations * commands.len()) as u32;

    assert!(average < std::time::Duration::from_millis(1), "average: {average:?}");
}
