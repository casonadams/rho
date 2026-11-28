use crate::permission::matcher::{expand_home, wildcard_match};
use test_case::test_case;

#[test_case("git status *", "git status", true; "trailing star matches bare prefix")]
#[test_case("git status *", "git status --porcelain", true; "trailing star matches args")]
#[test_case("git status *", "git statusx", false; "trailing star needs word boundary")]
#[test_case("git status *", "git stash", false; "different subcommand")]
#[test_case("npm run *", "npm run build --watch", true; "trailing star matches many args")]
#[test_case("npm run build", "npm run build", true; "exact match")]
#[test_case("npm run build", "npm run build --watch", false; "exact rejects extra args")]
#[test_case("ls*", "lsof", true; "glued star")]
#[test_case("* --version", "node --version", true; "leading star")]
#[test_case("git * main", "git push origin main", true; "middle star")]
#[test_case("a?c", "abc", true; "question mark single char")]
#[test_case("a?c", "abbc", false; "question mark rejects two chars")]
#[test_case("*", "anything at all", true; "star matches anything")]
#[test_case("", "", true; "empty matches empty")]
#[test_case("", "x", false; "empty rejects text")]
#[test_case("héllo *", "héllo wörld", true; "unicode")]
#[test_case("/tmp/*", "/tmp", true; "dir star matches dir")]
#[test_case("/tmp/*", "/tmp/file.txt", true; "dir star matches file")]
#[test_case("/tmp/*", "/tmp/sub/file.txt", true; "dir star matches nested")]
#[test_case("/tmp/*", "/tmpx", false; "dir star needs separator")]
fn wildcard_matching(pattern: &str, text: &str, expected: bool) {
    assert_eq!(wildcard_match(pattern, text), expected);
}

#[test_case("~/dir/file", "{home}/dir/file"; "tilde path")]
#[test_case("$HOME/dir/file", "{home}/dir/file"; "home var path")]
#[test_case("~", "{home}"; "bare tilde")]
#[test_case("$HOME", "{home}"; "bare home var")]
#[test_case("/var/log", "/var/log"; "absolute path unchanged")]
fn home_directory_expansion(input: &str, expected: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    assert_eq!(expand_home(input), expected.replace("{home}", &home));
}
