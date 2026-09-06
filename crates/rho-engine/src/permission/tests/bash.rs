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

#[test]
fn dynamic_execution_is_detected() {
    for command in ["echo $(whoami)", "echo `whoami`", "diff <(ls) <(ls -a)", "tee >(gzip)"] {
        assert!(analyze_bash_command(command).suspicious, "{command}");
    }
    assert!(!analyze_bash_command("git status --porcelain").suspicious);
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
