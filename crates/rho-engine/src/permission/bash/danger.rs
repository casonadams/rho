use regex::Regex;
use std::sync::LazyLock;

static CRITICAL_DANGER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(\brm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+([/~]|\.\.|\*)|mkfs|dd\s+if=|>+\s*/dev/sd|:\(\)\s*\{\s*:\|:&\s*\};:|chmod\s+-[a-zA-Z]*R\s+777|\bgit\s+clean\s+(-[a-zA-Z]*f[a-zA-Z]*d|-[a-zA-Z]*d[a-zA-Z]*f|-[a-zA-Z]*f\b.*-[a-zA-Z]*d|-[a-zA-Z]*d\b.*-[a-zA-Z]*f)|\bgit\s+reset\s+--hard\b|\bpsql\b.*(DROP\s+(DATABASE|SCHEMA|TABLE)|TRUNCATE\b|DELETE\s+FROM\b|ALTER\s+(TABLE|ROLE)\b)|\bkubectl\s+(delete\s+(all|namespace|ns\b)|drain\b|cordon\b)|\bgcloud\s+.*delete\b|\bgcloud\s+iam\s+.*delete)"#,
    )
    .expect("valid critical danger regex")
});

pub fn is_critical_danger_bash(cmd: &str) -> bool {
    CRITICAL_DANGER_REGEX.is_match(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_destructive_filesystem_commands() {
        assert!(is_critical_danger_bash("rm -rf /"));
        assert!(is_critical_danger_bash("rm -rf ~"));
        assert!(is_critical_danger_bash("rm -r /"));
        assert!(is_critical_danger_bash("rm -fr /"));
        assert!(is_critical_danger_bash("rm -rf .."));
        assert!(is_critical_danger_bash("rm -rf *"));
        assert!(is_critical_danger_bash("mkfs.ext4 /dev/sda1"));
        assert!(is_critical_danger_bash("dd if=/dev/zero of=/dev/sda"));
        assert!(is_critical_danger_bash("echo hi > /dev/sda"));
        assert!(is_critical_danger_bash("chmod -R 777 /"));
        assert!(is_critical_danger_bash(":(){ :|:& };:"));
    }

    #[test]
    fn matches_destructive_git_commands() {
        assert!(is_critical_danger_bash("git reset --hard"));
        assert!(is_critical_danger_bash("git reset --hard HEAD~1"));
        assert!(is_critical_danger_bash("git clean -fd"));
        assert!(is_critical_danger_bash("git clean -f -d"));
    }

    #[test]
    fn matches_destructive_database_and_cluster_commands() {
        assert!(is_critical_danger_bash("psql -U postgres -c 'DROP DATABASE prod;'"));
        assert!(is_critical_danger_bash("psql -c 'TRUNCATE users;'"));
        assert!(is_critical_danger_bash("kubectl delete namespace production"));
        assert!(is_critical_danger_bash("kubectl delete all --all"));
        assert!(is_critical_danger_bash("kubectl drain node-1"));
        assert!(is_critical_danger_bash("kubectl cordon node-1"));
        assert!(is_critical_danger_bash("gcloud compute instances delete prod-server"));
        assert!(is_critical_danger_bash(
            "gcloud iam service-accounts delete sa@proj.iam.gserviceaccount.com"
        ));
    }

    #[test]
    fn allows_safe_developer_commands() {
        assert!(!is_critical_danger_bash("cargo build"));
        assert!(!is_critical_danger_bash("cargo test --workspace"));
        assert!(!is_critical_danger_bash("git status"));
        assert!(!is_critical_danger_bash("git commit -m 'feat: clean commit'"));
        assert!(!is_critical_danger_bash("rm -rf target/debug"));
        assert!(!is_critical_danger_bash("rm -rf dist"));
        assert!(!is_critical_danger_bash("mkdir -p src/foo"));
        assert!(!is_critical_danger_bash("touch file.txt"));
        assert!(!is_critical_danger_bash("npm install"));
    }
}
