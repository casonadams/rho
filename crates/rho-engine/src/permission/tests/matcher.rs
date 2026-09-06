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
    let cases = [
        ("~/dir/file", format!("{home}/dir/file")),
        ("$HOME/dir/file", format!("{home}/dir/file")),
        ("~", home.clone()),
        ("$HOME", home),
        ("/var/log", "/var/log".to_string()),
    ];
    for (input, expected) in cases {
        assert_eq!(expand_home(input), expected);
    }
}
