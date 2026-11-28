use super::parser::GuardVerdict;
use crate::permission::bash::analyze_bash_command;
use regex::Regex;
use std::sync::LazyLock;

static CRITICAL_DANGER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(\brm\s+-[a-zA-Z]*r[a-zA-Z]*\s+([/~]|\.\.|\*)|:\s*\(\s*\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:|\bmkfs(?:\.[a-z0-9]+)?\b|\bdd\s+if=|>+\s*/dev/(?:sd|nvme|vd|disk)|\bgit\s+reset\s+--hard\b)",
    )
    .expect("valid critical danger regex")
});

static RM_WIPE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\brm\s+-[a-zA-Z]*r[a-zA-Z]*\s+([/~]|\.\.|\*)").expect("valid rm wipe regex"));
static FORK_BOMB_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*\(\s*\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:").expect("valid fork bomb regex"));
static MKFS_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bmkfs(?:\.[a-z0-9]+)?\b").expect("valid mkfs regex"));
static DD_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bdd\s+if=").expect("valid dd regex"));
static DEV_STORAGE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)>+\s*/dev/(?:sd|nvme|vd|disk)").expect("valid dev storage regex"));
static GIT_RESET_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bgit\s+reset\s+--hard\b").expect("valid git reset regex"));

const WIPE_ACTION: (&str, &str) = (
    "Recursively deletes root, home, parent, or wildcard files",
    "Critical destructive filesystem wipe detected.",
);

fn is_recursive_flag(token: &str) -> bool {
    token == "--recursive" || (token.starts_with('-') && !token.starts_with("--") && token.contains(['r', 'R']))
}

fn is_wipe_target(token: &str) -> bool {
    let trimmed = if token.len() > 1 {
        token.trim_end_matches('/')
    } else {
        token
    };
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    matches!(
        trimmed,
        "/" | "~" | ".." | "*" | "/*" | "~/*" | "$HOME" | "${HOME}" | "$HOME/*" | "${HOME}/*"
    )
}

/// Judges `rm` on its arguments, so split flags (`-r -f`), long options and quoted or
/// variable targets are caught the same way as `rm -rf /`.
fn rm_wipes_argv(segment: &str) -> bool {
    let mut tokens = segment.split_whitespace();
    if tokens.next() != Some("rm") {
        return false;
    }
    let args: Vec<&str> = tokens.collect();
    let recursive = args.iter().take_while(|a| **a != "--").any(|a| is_recursive_flag(a));
    recursive && args.iter().any(|a| is_wipe_target(a))
}

fn classify(text: &str) -> Option<(&'static str, &'static str)> {
    if !CRITICAL_DANGER_REGEX.is_match(text) {
        return None;
    }
    if RM_WIPE_REGEX.is_match(text) {
        return Some(WIPE_ACTION);
    }
    if FORK_BOMB_REGEX.is_match(text) {
        return Some((
            "Executes a shell fork bomb",
            "Catastrophic system denial of service / resource exhaustion.",
        ));
    }
    if MKFS_REGEX.is_match(text) {
        return Some((
            "Formats disk partition or builds a new filesystem",
            "Critical destructive drive format detected.",
        ));
    }
    if DD_REGEX.is_match(text) {
        return Some((
            "Performs low-level raw byte copying or writing with dd",
            "Potential low-level raw disk or partition overwrite.",
        ));
    }
    if DEV_STORAGE_REGEX.is_match(text) {
        return Some((
            "Redirects output directly into storage device node",
            "Direct raw storage device overwrite detected.",
        ));
    }
    if GIT_RESET_REGEX.is_match(text) {
        return Some((
            "Resets git working tree and index discarding uncommitted changes",
            "Irreversible loss of git working tree state and uncommitted changes.",
        ));
    }
    Some((
        "Executes high-risk destructive shell command",
        "Critical destructive or irreversible infrastructure action detected.",
    ))
}

/// Checks the raw text, then each command segment with quotes, paths and wrappers normalized away.
pub fn check_critical_danger(command: &str) -> Option<GuardVerdict> {
    let analysis = analyze_bash_command(command);
    let hit = classify(command).or_else(|| {
        analysis
            .normalized_commands
            .iter()
            .find_map(|segment| classify(segment).or_else(|| rm_wipes_argv(segment).then_some(WIPE_ACTION)))
    })?;
    Some(GuardVerdict {
        safe: false,
        action: Some(hit.0.to_string()),
        reason: hit.1.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn catches_destructive_rm_patterns() {
        let targets = ["rm -rf /", "rm -r ~", "rm -rf ..", "rm -rf *", "rm -rf /var/log"];
        for cmd in targets {
            let verdict = check_critical_danger(cmd).expect("should intercept destructive rm");
            assert!(!verdict.safe);
            assert!(verdict.reason.contains("filesystem wipe"));
            assert!(verdict.action.unwrap().contains("Recursively deletes"));
        }
    }

    #[test]
    fn catches_obfuscated_and_split_rm_forms() {
        let targets = [
            "rm -r -f /",
            "rm -rf \"/\"",
            "\"rm\" -rf /",
            "rm -rf \"$HOME\"",
            "rm -rf $HOME",
            "rm -rf ${HOME}/",
            "rm --recursive --force /",
            "rm -Rf ~",
            "/bin/rm -rf /",
            "sudo rm -rf /",
            "env rm -fr /",
            "rm -rf -- /",
            "rm / -rf",
            "echo ok && rm -r -f \"..\"",
            "git status; rm -rf /*",
            "\"git\" reset --hard HEAD",
        ];
        for cmd in targets {
            assert!(check_critical_danger(cmd).is_some(), "{cmd}");
        }
    }

    fn intercepted(cmd: &str) -> GuardVerdict {
        let verdict = check_critical_danger(cmd).unwrap_or_else(|| panic!("should intercept {cmd}"));
        assert!(!verdict.safe, "{cmd}");
        verdict
    }

    #[test_case(":(){ :|:& };:", "denial of service"; "fork bomb")]
    #[test_case("mkfs.ext4 /dev/sda1", "destructive drive format"; "mkfs")]
    #[test_case("dd if=/dev/zero of=/dev/sda", "raw disk or partition overwrite"; "dd to disk")]
    #[test_case("echo evil > /dev/sda", "raw storage device overwrite"; "redirect to disk")]
    #[test_case("cat file >> /dev/nvme0n1", "raw storage device overwrite"; "append to nvme")]
    #[test_case("git reset --hard HEAD~1", "git working tree state"; "git reset hard")]
    fn catches_destructive_commands_by_their_reason(cmd: &str, reason: &str) {
        let verdict = intercepted(cmd);
        assert!(verdict.reason.contains(reason), "{}", verdict.reason);
    }

    #[test_case(":(){ :|:& };:", "Executes a shell fork bomb"; "fork bomb")]
    #[test_case(
        "git reset --hard HEAD~1",
        "Resets git working tree and index discarding uncommitted changes";
        "git reset hard"
    )]
    fn critical_commands_describe_their_action(cmd: &str, action: &str) {
        assert_eq!(intercepted(cmd).action.as_deref(), Some(action));
    }

    #[test]
    fn ordinary_commands_are_not_intercepted() {
        let ok = [
            "rm -r build",
            "rm -rf ./target",
            "rm -rf \"$HOME/project/tmp\"",
            "rm -f /tmp/a.log",
            "rm /",
            "rm -- -r /",
            "rm -rf target",
            "rm file.txt",
            "rm -f test.log",
            "git status",
            "git reset HEAD file.txt",
            "cargo test",
            "echo 'hello world'",
        ];
        for cmd in ok {
            assert!(check_critical_danger(cmd).is_none(), "{cmd}");
        }
    }
}
