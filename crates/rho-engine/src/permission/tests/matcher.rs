use crate::permission::matcher::{expand_home, wildcard_match};

#[test]
fn wildcard_matching() {
    let cases = [
        ("git status *", "git status", true),
        ("git status *", "git status --porcelain", true),
        ("git status *", "git statusx", false),
        ("git status *", "git stash", false),
        ("npm run *", "npm run build --watch", true),
        ("npm run build", "npm run build", true),
        ("npm run build", "npm run build --watch", false),
        ("ls*", "lsof", true),
        ("* --version", "node --version", true),
        ("git * main", "git push origin main", true),
        ("a?c", "abc", true),
        ("a?c", "abbc", false),
        ("*", "anything at all", true),
        ("", "", true),
        ("", "x", false),
        ("héllo *", "héllo wörld", true),
        ("/tmp/*", "/tmp", true),
        ("/tmp/*", "/tmp/file.txt", true),
        ("/tmp/*", "/tmp/sub/file.txt", true),
        ("/tmp/*", "/tmpx", false),
    ];
    for (pattern, text, expected) in cases {
        assert_eq!(wildcard_match(pattern, text), expected, "{pattern:?} vs {text:?}");
    }
}

#[test]
fn home_directory_expansion() {
    let home = std::env::var("HOME").unwrap_or_default();
    assert_eq!(expand_home("~/dir/file"), format!("{home}/dir/file"));
    assert_eq!(expand_home("$HOME/dir/file"), format!("{home}/dir/file"));
    assert_eq!(expand_home("~"), home);
    assert_eq!(expand_home("$HOME"), home);
    assert_eq!(expand_home("/var/log"), "/var/log");
}
