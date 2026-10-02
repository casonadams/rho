use super::parser::GuardVerdict;
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

pub fn check_critical_danger(command: &str) -> Option<GuardVerdict> {
    if !CRITICAL_DANGER_REGEX.is_match(command) {
        return None;
    }

    let (action, reason) = if RM_WIPE_REGEX.is_match(command) {
        (
            "Recursively deletes root, home, parent, or wildcard files",
            "Critical destructive filesystem wipe detected.",
        )
    } else if FORK_BOMB_REGEX.is_match(command) {
        (
            "Executes a shell fork bomb",
            "Catastrophic system denial of service / resource exhaustion.",
        )
    } else if MKFS_REGEX.is_match(command) {
        (
            "Formats disk partition or builds a new filesystem",
            "Critical destructive drive format detected.",
        )
    } else if DD_REGEX.is_match(command) {
        (
            "Performs low-level raw byte copying or writing with dd",
            "Potential low-level raw disk or partition overwrite.",
        )
    } else if DEV_STORAGE_REGEX.is_match(command) {
        (
            "Redirects output directly into storage device node",
            "Direct raw storage device overwrite detected.",
        )
    } else if GIT_RESET_REGEX.is_match(command) {
        (
            "Resets git working tree and index discarding uncommitted changes",
            "Irreversible loss of git working tree state and uncommitted changes.",
        )
    } else {
        (
            "Executes high-risk destructive shell command",
            "Critical destructive or irreversible infrastructure action detected.",
        )
    };

    Some(GuardVerdict {
        safe: false,
        action: Some(action.to_string()),
        reason: reason.to_string(),
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
