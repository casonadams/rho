use crate::permission::bash::TokenKind;
use crate::permission::bash::lexer::tokenize;
use crate::permission::bash::{analyze_bash_command, format_command_lines, has_file_redirection};

#[test]
fn bash_lexer_tokenization_quotes() {
    let res = tokenize("echo 'hello world' \"foo $bar\"");
    let actual = (
        res.tokens.len(),
        res.tokens[0].text.as_str(),
        res.tokens[1].text.as_str(),
        res.tokens[2].text.as_str(),
        res.suspicious,
    );
    assert_eq!(actual, (3, "echo", "hello world", "foo $bar", false));
}

#[test]
fn bash_lexer_suspicious_patterns() {
    for cmd in ["echo `whoami`", "echo $(id)", "echo 'unterminated", "diff <(ls) >(cat)"] {
        assert!(tokenize(cmd).suspicious, "{cmd}");
    }
}

#[test]
fn bash_analyzer_single_command() {
    let analysis = analyze_bash_command("grep \"a && b\" src/file.txt");
    assert_eq!(analysis.commands, vec!["grep \"a && b\" src/file.txt"]);
    assert_eq!(analysis.path_tokens, vec!["src/file.txt"]);
}

#[test]
fn bash_analyzer_env_and_timeout() {
    let analysis = analyze_bash_command("RUST_LOG=debug FOO=/tmp/x cargo test --nocapture");
    assert_eq!(analysis.commands, vec!["cargo test --nocapture"]);
    assert_eq!(analysis.path_tokens, vec!["/tmp/x"]);

    let analysis = analyze_bash_command("time timeout 10s cargo test");
    assert_eq!(analysis.commands, vec!["cargo test"]);

    let analysis = analyze_bash_command("rtk git status");
    assert_eq!(analysis.commands, vec!["git status"]);

    let analysis = analyze_bash_command("rtk read src/main.rs");
    assert_eq!(analysis.commands, vec!["cat src/main.rs"]);
    assert_eq!(analysis.path_tokens, vec!["src/main.rs"]);

    let analysis = analyze_bash_command("rtk gain");
    assert_eq!(analysis.commands, vec!["rtk gain"]);
}

#[test]
fn bash_analyzer_compound_and_redirect_commands() {
    let analysis = analyze_bash_command("cargo test > /tmp/out.log 2>&1");
    assert_eq!(analysis.commands, vec!["cargo test > /tmp/out.log 2>&1"]);
    assert_eq!(analysis.path_tokens, vec!["/tmp/out.log"]);

    let analysis = analyze_bash_command("git status && cargo test");
    assert_eq!(analysis.commands, vec!["git status", "cargo test"]);

    let analysis = analyze_bash_command("ls ~");
    assert_eq!(analysis.path_tokens, vec!["~"]);
}

#[test]
fn complex_commands_format_multiline_for_display() {
    let cases = [
        (
            "git status && cargo test || echo fallback ; ls -la",
            "git status &&\n  cargo test ||\n  echo fallback;\nls -la",
        ),
        ("cat a ; cat b ; cat c", "cat a;\ncat b;\ncat c"),
        ("cargo test --lib", "cargo test --lib"),
        ("grep \"a && b\" src/file.txt", "grep \"a && b\" src/file.txt"),
        ("ls\ncat foo", "ls\ncat foo"),
    ];
    for (input, expected) in cases {
        assert_eq!(format_command_lines(input), expected);
    }
}

#[test]
fn bash_commands_split_on_operators() {
    let analysis = analyze_bash_command("git status && npm test | grep foo ; rm -rf tmp\nls");
    assert_eq!(
        analysis.commands,
        ["git status", "npm test", "grep foo", "rm -rf tmp", "ls"]
    );
}

fn assert_all_suspicious(commands: &[&str]) {
    for &command in commands {
        assert!(analyze_bash_command(command).suspicious, "{command}");
    }
}

#[test]
fn dynamic_execution_is_detected() {
    assert_all_suspicious(&["echo $(whoami)", "echo `whoami`", "diff <(ls) <(ls -a)", "tee >(gzip)"]);
    assert!(!analyze_bash_command("git status --porcelain").suspicious);
}

#[test]
fn leading_env_assignment_marks_command_suspicious() {
    let analysis = analyze_bash_command("GIT_PAGER='sh -c id' git log");
    assert_eq!(analysis.commands, vec!["git log"]);
    assert!(analysis.suspicious);
    assert!(!analyze_bash_command("git log").suspicious);
}

#[test]
fn filesystem_root_and_option_values_are_path_tokens() {
    assert!(analyze_bash_command("rm -rf /").path_tokens.contains(&"/".to_string()));
    assert_eq!(
        analyze_bash_command("tool --output=/etc/passwd").path_tokens,
        vec!["/etc/passwd"]
    );
    assert_eq!(
        analyze_bash_command("cc -I/usr/include").path_tokens,
        vec!["/usr/include"]
    );
}

#[test]
fn expansions_and_brace_lists_are_suspicious() {
    assert_all_suspicious(&["cat ${HOME}/x", "cat $TMPDIR/x", "cat ~root/x", "cat {/etc/passwd,x}"]);
    assert!(!analyze_bash_command("cat ~/x").suspicious);
    assert!(!analyze_bash_command("find . -exec wc {} +").suspicious);
}

#[test]
fn redirection_is_detected() {
    for command in [
        "echo hi > f",
        "echo hi >> f",
        "sort < x > y",
        "cmd &> f",
        "cmd 2>f",
        "git log > f 2>&1",
    ] {
        assert!(has_file_redirection(command), "{command}");
    }
    for command in ["ls 2>&1", "cmd 1>&2", ">&2 echo x", "grep x f", "no redirects here"] {
        assert!(!has_file_redirection(command), "{command}");
    }
}

fn commands(command: &str) -> Vec<String> {
    analyze_bash_command(command).commands
}

fn paths(command: &str) -> Vec<String> {
    analyze_bash_command(command).path_tokens
}

fn raws(command: &str) -> Vec<(TokenKind, String)> {
    tokenize(command).tokens.into_iter().map(|t| (t.kind, t.raw)).collect()
}

#[test]
fn lexer_recognizes_two_character_operators_and_ampersand_redirects() {
    use TokenKind::{Redirect, Separator, Word};
    let word = |s: &str| (Word, s.to_string());
    assert_eq!(raws("a |& b"), [word("a"), (Separator, "|&".into()), word("b")]);
    assert_eq!(raws("a ;; b"), [word("a"), (Separator, ";;".into()), word("b")]);
    assert_eq!(raws("a && b"), [word("a"), (Separator, "&&".into()), word("b")]);
    assert_eq!(raws("a || b"), [word("a"), (Separator, "||".into()), word("b")]);
    assert_eq!(raws("a &> f"), [word("a"), (Redirect, "&>".into()), word("f")]);
    assert_eq!(raws("a &>> f"), [word("a"), (Redirect, "&>>".into()), word("f")]);
    assert_eq!(raws("a & b"), [word("a"), (Separator, "&".into()), word("b")]);
    assert_eq!(raws("a >"), [word("a"), (Redirect, ">".into())]);
}

#[test]
fn redirect_targets_are_path_tokens_but_fd_duplication_is_not() {
    assert_eq!(paths("echo hi > out.txt"), ["out.txt"]);
    assert_eq!(paths("echo hi >> /etc/x"), ["/etc/x"]);
    assert_eq!(paths("echo hi > a > a"), ["a"]);
    assert_eq!(paths("echo hi 2>&1"), Vec::<String>::new());
    assert_eq!(paths("echo hi >"), Vec::<String>::new());
    assert_eq!(paths("> /tmp/f"), ["/tmp/f"]);
}

#[test]
fn file_redirection_ignores_only_fd_duplication() {
    assert!(has_file_redirection("echo hi > f"));
    assert!(has_file_redirection("echo hi >"));
    assert!(has_file_redirection("echo hi 2>&1 > f"));
    assert!(!has_file_redirection("echo hi 2>&1"));
    assert!(!has_file_redirection("echo hi >&2"));
    assert!(!has_file_redirection("echo hi"));
}

#[test]
fn redirect_only_segments_have_no_command() {
    let analysis = analyze_bash_command("> /tmp/f");
    assert!(analysis.commands.is_empty());
    assert!(analysis.suspicious);
}

#[test]
fn assignments_and_timeout_are_stripped_only_when_a_command_follows() {
    assert_eq!(commands("FOO=/tmp/x"), ["FOO=/tmp/x"]);
    assert_eq!(commands("FOO=1 BAR=2 ls"), ["ls"]);
    assert_eq!(commands("1A=x ls"), ["1A=x ls"]);
    assert_eq!(commands("A-B=x ls"), ["A-B=x ls"]);
    assert_eq!(commands("=x ls"), ["=x ls"]);
    assert_eq!(commands("timeout 5"), ["timeout 5"]);
    assert_eq!(commands("timeout 5s ls"), ["ls"]);
    assert_eq!(commands("timeout abc ls"), ["timeout abc ls"]);
    assert_eq!(commands("timeout"), ["timeout"]);
}

#[test]
fn path_tokens_skip_options_urls_and_scoped_packages() {
    assert_eq!(paths("cat -n"), Vec::<String>::new());
    assert_eq!(paths("curl https://example.com/a"), Vec::<String>::new());
    assert_eq!(paths("npm i @scope/pkg"), Vec::<String>::new());
    assert_eq!(paths("cat @/x"), ["@/x"]);
    assert_eq!(paths("cat ../x a/b ./c"), ["../x", "a/b", "./c"]);
    assert_eq!(paths("tool --out=a/b"), ["a/b"]);
    assert_eq!(paths("tool -x"), Vec::<String>::new());
    assert_eq!(paths("tool K=a/b"), Vec::<String>::new());
}

#[test]
fn trailing_and_leading_operators_make_a_command_suspicious() {
    for command in ["ls &&", "ls ||", "ls |", "ls &", "&& ls", "; ls", "ls && && ls"] {
        assert!(analyze_bash_command(command).suspicious, "{command}");
    }
    for command in ["ls;", "ls\n", "ls ; ls", "ls\nls"] {
        assert!(!analyze_bash_command(command).suspicious, "{command}");
    }
}

#[test]
fn operator_formatting_breaks_and_indents_each_continuation() {
    let cases = [
        ("a && b", "a &&\n  b"),
        ("a || b", "a ||\n  b"),
        ("a | b", "a |\n  b"),
        ("a & b", "a &\n  b"),
        ("a ; b", "a;\nb"),
        ("&& a", "&&\n  a"),
        ("a", "a"),
        ("a\nb", "a\nb"),
    ];
    for (input, expected) in cases {
        assert_eq!(format_command_lines(input), expected, "{input}");
    }
}

#[test]
fn quoted_ampersand_digit_filenames_are_file_redirections() {
    for command in ["echo hi > '&1'", "echo hi > \"&2\"", "echo hi >> '&1'"] {
        assert!(has_file_redirection(command), "{command}");
    }
    assert_eq!(paths("echo hi > '&1'"), ["&1"]);
}

#[test]
fn formatting_keeps_single_line_and_newline_only_commands_verbatim() {
    assert_eq!(format_command_lines("ls ;"), "ls ;");
    assert_eq!(format_command_lines("ls   -la\ncat"), "ls   -la\ncat");
}
