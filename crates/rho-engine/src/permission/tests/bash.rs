use crate::permission::bash::lexer::tokenize;
use crate::permission::bash::{analyze_bash_command, format_command_lines, has_file_redirection};

#[test]
fn bash_lexer_tokenization_and_quotes() {
    let res = tokenize("echo 'hello world' \"foo $bar\"");
    assert_eq!(res.tokens.len(), 3);
    assert_eq!(res.tokens[0].text, "echo");
    assert_eq!(res.tokens[1].text, "hello world");
    assert_eq!(res.tokens[2].text, "foo $bar");
    assert!(!res.suspicious);

    assert!(tokenize("echo `whoami`").suspicious);
    assert!(tokenize("echo $(id)").suspicious);
    assert!(tokenize("echo 'unterminated").suspicious);
    assert!(tokenize("diff <(ls) >(cat)").suspicious);
}

#[test]
fn bash_analyzer_command_and_paths() {
    let analysis = analyze_bash_command("grep \"a && b\" src/file.txt");
    assert_eq!(analysis.commands, vec!["grep \"a && b\" src/file.txt"]);
    assert_eq!(analysis.path_tokens, vec!["src/file.txt"]);
    assert!(!analysis.suspicious);

    let analysis = analyze_bash_command("RUST_LOG=debug FOO=/tmp/x cargo test --nocapture");
    assert_eq!(analysis.commands, vec!["cargo test --nocapture"]);
    assert_eq!(analysis.path_tokens, vec!["/tmp/x"]);
    assert!(!analysis.suspicious);

    let analysis = analyze_bash_command("time timeout 10s cargo test");
    assert_eq!(analysis.commands, vec!["cargo test"]);
    assert!(!analysis.suspicious);

    let analysis = analyze_bash_command("cargo test > /tmp/out.log 2>&1");
    assert_eq!(analysis.commands, vec!["cargo test > /tmp/out.log 2>&1"]);
    assert_eq!(analysis.path_tokens, vec!["/tmp/out.log"]);
    assert!(!analysis.suspicious);

    let analysis = analyze_bash_command("git status && cargo test");
    assert_eq!(analysis.commands, vec!["git status", "cargo test"]);
    assert!(!analysis.suspicious);

    let analysis = analyze_bash_command("ls ~");
    assert_eq!(analysis.path_tokens, vec!["~"]);
}

#[test]
fn complex_commands_format_multiline_for_display() {
    assert_eq!(
        format_command_lines("git status && cargo test || echo fallback ; ls -la"),
        "git status\n  && cargo test\n  || echo fallback;\nls -la"
    );
    assert_eq!(format_command_lines("cat a ; cat b ; cat c"), "cat a;\ncat b;\ncat c");
    assert_eq!(format_command_lines("cargo test --lib"), "cargo test --lib");
    assert_eq!(
        format_command_lines("grep \"a && b\" src/file.txt"),
        "grep \"a && b\" src/file.txt"
    );
    assert_eq!(format_command_lines("ls\ncat foo"), "ls\ncat foo");
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
