//! Command-classification rules.
//!
//! This module owns everything that's *heuristic knowledge* about which shell
//! invocations and argument patterns are safe:
//!
//! - the read-only allow-list ([`READ_ONLY_COMMANDS`])
//! - the high-risk command set ([`is_high_risk_command`])
//! - the read-only carve-outs for `git` and `cargo`
//! - the output-redirection classifier ([`classify_output_redirect`],
//!   [`classify_output_path`])
//! - small argument-helpers ([`has_any`], [`has_short_flag`], [`contains_glob`],
//!   [`unquote`])
//!
//! The classifier functions all take `&mut Analyzer` so they can record reasons
//! and bump the tier; the `Analyzer` itself lives in [`super::analyzer`].

use super::analyzer::Analyzer;
use super::types::RiskTier;
use brush_parser::ast::{IoFileRedirectKind, IoFileRedirectTarget};

/// Shell commands that are unconditionally read-only — they cannot mutate any
/// state regardless of arguments. Anything not on this list is treated as
/// potentially mutating unless [`classify_invocation`] proves otherwise
/// (e.g. `git status`, `cargo check`).
pub(super) const READ_ONLY_COMMANDS: &[&str] = &[
    "[", "test", "true", "false", "ls", "dir", "pwd", "cat", "head", "tail", "grep", "rg", "ag", "fd", "tree", "wc",
    "stat", "file", "which", "whereis", "type", "echo", "printf", "printenv", "uname", "sw_vers", "df", "du", "ps",
    "whoami", "uptime", "diff", "cmp", "sort", "uniq", "cut", "tr", "basename", "dirname", "realpath", "readlink",
];

/// Classify an invocation given its resolved command name and arguments.
pub(super) fn classify_invocation(analyzer: &mut Analyzer, command: &str, arguments: &[String]) {
    if is_high_risk_command(command, arguments) {
        analyzer.flag(
            RiskTier::HighRisk,
            format!("HIGH RISK: destructive or elevated command: {command}"),
        );
        return;
    }
    match command {
        "eval" | "exec" | "source" | "." => analyzer.flag(
            RiskTier::Mutating,
            format!("Dynamic shell execution requires approval: {command}"),
        ),
        "cd" if matches!(arguments, [path] if matches!(path.as_str(), "." | "./")) => {}
        "cd" => analyzer.flag(
            RiskTier::Mutating,
            "Changing the shell working directory requires approval",
        ),
        "find" if has_any(arguments, &["-delete", "-exec", "-execdir", "-ok", "-okdir"]) => analyzer.flag(
            RiskTier::Mutating,
            "find invocation can execute commands or delete files",
        ),
        "find" => {}
        "git" if is_read_only_git(arguments) => {}
        "git" => analyzer.flag(RiskTier::Mutating, "Git invocation may modify repository state"),
        "cargo" if is_read_only_cargo(arguments) => {}
        "cargo" => analyzer.flag(
            RiskTier::Mutating,
            "Cargo invocation may execute or modify project state",
        ),
        "tee" if arguments.is_empty() || arguments.iter().all(|arg| arg == "/dev/null") => {}
        command if READ_ONLY_COMMANDS.contains(&command) => {}
        command => analyzer.flag(
            RiskTier::Mutating,
            format!("Command is not on the verified read-only list: {command}"),
        ),
    }
}

pub(super) fn classify_output_redirect(
    analyzer: &mut Analyzer,
    kind: &IoFileRedirectKind,
    target: &IoFileRedirectTarget,
) {
    match target {
        IoFileRedirectTarget::Fd(_) | IoFileRedirectTarget::Duplicate(_) => {}
        IoFileRedirectTarget::Filename(word) => analyzer.classify_output_path(&word.value),
        IoFileRedirectTarget::ProcessSubstitution(_, _) => {}
    }
    if matches!(kind, IoFileRedirectKind::ReadAndWrite) {
        analyzer.flag(RiskTier::Mutating, "Read/write redirection may modify its target");
    }
}

pub(super) fn classify_output_path(analyzer: &mut Analyzer, raw: &str) {
    let path = unquote(raw);
    if matches!(path.as_str(), "/dev/null" | "/dev/stdout" | "/dev/stderr") {
        return;
    }
    if path.starts_with("/dev/") || path.starts_with("/etc/") {
        analyzer.flag(
            RiskTier::HighRisk,
            format!("HIGH RISK: output redirection targets sensitive path {path}"),
        );
    } else {
        analyzer.flag(
            RiskTier::Mutating,
            format!("Writes output through file redirection to {path}"),
        );
    }
}

/// Commands that require approval regardless of arguments because they are
/// destructive or elevate privileges.
fn is_high_risk_command(command: &str, arguments: &[String]) -> bool {
    match command {
        "sudo" | "doas" | "su" | "mkfs" | "fdisk" | "shred" => true,
        "dd" => true,
        "rm" => {
            has_short_flag(arguments, 'r')
                || has_short_flag(arguments, 'R')
                || has_short_flag(arguments, 'f')
                || has_any(arguments, &["--recursive", "--force"])
        }
        "unlink" => arguments.iter().any(|arg| contains_glob(arg)),
        "git" => is_high_risk_git(arguments),
        _ => false,
    }
}

/// Patterns that make a `git` invocation destructive enough to be
/// [`RiskTier::HighRisk`].
fn is_high_risk_git(arguments: &[String]) -> bool {
    match arguments.first().map(String::as_str) {
        Some("reset") => has_any(arguments, &["--hard"]),
        Some("clean") => has_short_flag(arguments, 'f') || has_any(arguments, &["--force"]),
        Some("push") => has_any(arguments, &["--force", "--force-with-lease"]) || has_short_flag(arguments, 'f'),
        Some("checkout") => arguments.windows(2).any(|args| args == ["--", "."]),
        Some("restore") => arguments.iter().any(|arg| arg == "."),
        _ => false,
    }
}

/// `git` subcommands that are verifiably read-only.
fn is_read_only_git(arguments: &[String]) -> bool {
    let Some(subcommand) = arguments.first().map(String::as_str) else {
        return false;
    };
    match subcommand {
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files" | "describe" | "--version" => true,
        "branch" => arguments[1..]
            .iter()
            .all(|arg| matches!(arg.as_str(), "-a" | "-r" | "-v" | "-vv" | "--list" | "--show-current")),
        "tag" => arguments[1..].iter().all(|arg| matches!(arg.as_str(), "-l" | "--list")),
        "remote" => {
            arguments.len() == 1
                || matches!(&arguments[1..], [arg] if arg == "-v")
                || matches!(&arguments[1..], [action, _] if action == "get-url")
        }
        "config" => {
            arguments.len() == 2
                || arguments.get(1).is_some_and(|arg| {
                    matches!(
                        arg.as_str(),
                        "--get" | "--get-all" | "--get-regexp" | "--list" | "-l" | "--show-origin"
                    )
                })
        }
        _ => false,
    }
}

/// `cargo` subcommands that are verifiably read-only (no execution, no writes
/// outside `target/`).
fn is_read_only_cargo(arguments: &[String]) -> bool {
    arguments.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "check" | "clippy" | "test" | "tree" | "metadata" | "--version" | "version"
        )
    })
}

fn has_any(arguments: &[String], values: &[&str]) -> bool {
    arguments.iter().any(|arg| values.contains(&arg.as_str()))
}

fn has_short_flag(arguments: &[String], flag: char) -> bool {
    arguments
        .iter()
        .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(flag))
}

fn contains_glob(value: &str) -> bool {
    value.contains(['*', '?', '['])
}

fn unquote(value: &str) -> String {
    value.trim_matches(['\'', '"']).to_string()
}
