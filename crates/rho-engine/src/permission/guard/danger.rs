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

    #[test]
    fn argv_check_leaves_ordinary_rm_alone() {
        let ok = [
            "rm -r build",
            "rm -rf ./target",
            "rm -rf \"$HOME/project/tmp\"",
            "rm -f /tmp/a.log",
            "rm /",
            "rm -- -r /",
        ];
        for cmd in ok {
            assert!(check_critical_danger(cmd).is_none(), "{cmd}");
        }
    }

    #[test]
    fn allows_safe_rm_target() {
        assert!(check_critical_danger("rm -rf target").is_none());
        assert!(check_critical_danger("rm file.txt").is_none());
        assert!(check_critical_danger("rm -f test.log").is_none());
    }

    #[test]
    fn catches_fork_bomb() {
        let cmd = ":(){ :|:& };:";
        let verdict = check_critical_danger(cmd).expect("should intercept fork bomb");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("denial of service"));
        assert_eq!(verdict.action.as_deref(), Some("Executes a shell fork bomb"));
    }

    #[test]
    fn catches_disk_format_mkfs() {
        let verdict = check_critical_danger("mkfs.ext4 /dev/sda1").expect("should intercept mkfs");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("destructive drive format"));
    }

    #[test]
    fn catches_dd_if() {
        let verdict = check_critical_danger("dd if=/dev/zero of=/dev/sda").expect("should intercept dd");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("raw disk or partition overwrite"));
    }

    #[test]
    fn catches_dev_storage_redirection() {
        let verdict = check_critical_danger("echo evil > /dev/sda").expect("should intercept dev redirection");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("raw storage device overwrite"));

        let nvme = check_critical_danger("cat file >> /dev/nvme0n1").expect("should intercept nvme");
        assert!(!nvme.safe);
    }

    #[test]
    fn catches_git_reset_hard() {
        let verdict = check_critical_danger("git reset --hard HEAD~1").expect("should intercept git reset --hard");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("git working tree state"));
        assert_eq!(
            verdict.action.as_deref(),
            Some("Resets git working tree and index discarding uncommitted changes")
        );
    }

    #[test]
    fn allows_benign_commands() {
        assert!(check_critical_danger("git status").is_none());
        assert!(check_critical_danger("git reset HEAD file.txt").is_none());
        assert!(check_critical_danger("cargo test").is_none());
        assert!(check_critical_danger("echo 'hello world'").is_none());
    }
}
