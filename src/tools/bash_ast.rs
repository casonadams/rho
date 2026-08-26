use brush_parser::ast::{
    Command, CommandPrefixOrSuffixItem, CompoundCommand, CompoundList, ExtendedTestExpr, IoFileRedirectKind,
    IoFileRedirectTarget, IoRedirect, RedirectList, SimpleCommand, Word,
};
use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use brush_parser::{Parser, ParserOptions};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

const MAX_AST_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskTier {
    ReadOnly,
    Mutating,
    HighRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyAnalysis {
    pub tier: RiskTier,
    pub reasons: Vec<String>,
    pub commands: Vec<String>,
}

pub fn analyze_command_safety(command: &str) -> SafetyAnalysis {
    if command.trim().is_empty() {
        return SafetyAnalysis {
            tier: RiskTier::Mutating,
            reasons: vec!["Empty shell input cannot be proven read-only".to_string()],
            commands: Vec::new(),
        };
    }

    let options = ParserOptions::default();
    let mut parser = Parser::new(Cursor::new(command.as_bytes()), &options);
    let Ok(program) = parser.parse_program() else {
        return SafetyAnalysis {
            tier: RiskTier::Mutating,
            reasons: vec!["Shell AST parse error; approval is required".to_string()],
            commands: Vec::new(),
        };
    };

    let mut analyzer = Analyzer::new(options);
    for complete_command in &program.complete_commands {
        analyzer.visit_list(complete_command, 0);
    }
    analyzer.finish()
}

struct Analyzer {
    options: ParserOptions,
    tier: RiskTier,
    reasons: Vec<String>,
    commands: Vec<String>,
}

impl Analyzer {
    fn new(options: ParserOptions) -> Self {
        Self {
            options,
            tier: RiskTier::ReadOnly,
            reasons: Vec::new(),
            commands: Vec::new(),
        }
    }

    fn finish(mut self) -> SafetyAnalysis {
        self.reasons.sort();
        self.reasons.dedup();
        self.commands.dedup();
        SafetyAnalysis {
            tier: self.tier,
            reasons: self.reasons,
            commands: self.commands,
        }
    }

    fn flag(&mut self, tier: RiskTier, reason: impl Into<String>) {
        self.tier = self.tier.max(tier);
        self.reasons.push(reason.into());
    }

    fn check_depth(&mut self, depth: usize) -> bool {
        if depth <= MAX_AST_DEPTH {
            true
        } else {
            self.flag(
                RiskTier::Mutating,
                "Shell AST nesting exceeds the safety analysis limit",
            );
            false
        }
    }

    fn visit_list(&mut self, list: &CompoundList, depth: usize) {
        if !self.check_depth(depth) {
            return;
        }
        for item in &list.0 {
            for (_, pipeline) in &item.0 {
                for command in &pipeline.seq {
                    self.visit_command(command, depth + 1);
                }
            }
        }
    }

    fn visit_command(&mut self, command: &Command, depth: usize) {
        if !self.check_depth(depth) {
            return;
        }
        match command {
            Command::Simple(simple) => self.visit_simple(simple, depth + 1),
            Command::Compound(compound, redirects) => {
                self.visit_compound(compound, depth + 1);
                if let Some(redirects) = redirects {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
            Command::ExtendedTest(test, redirects) => {
                self.visit_extended_test(&test.expr, depth + 1);
                if let Some(redirects) = redirects {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
            Command::Function(function) => {
                self.flag(
                    RiskTier::Mutating,
                    format!(
                        "Shell function definition is dynamically callable: {}",
                        function.fname.value
                    ),
                );
                self.visit_compound(&function.body.0, depth + 1);
                if let Some(redirects) = &function.body.1 {
                    self.visit_redirects(redirects, depth + 1);
                }
            }
        }
    }

    fn visit_compound(&mut self, command: &CompoundCommand, depth: usize) {
        if !self.check_depth(depth) {
            return;
        }
        match command {
            CompoundCommand::BraceGroup(group) => self.visit_list(&group.list, depth + 1),
            CompoundCommand::Subshell(subshell) => self.visit_list(&subshell.list, depth + 1),
            CompoundCommand::ForClause(clause) => {
                if let Some(values) = &clause.values {
                    for value in values {
                        self.inspect_argument_word(value, depth + 1);
                    }
                }
                self.visit_list(&clause.body.list, depth + 1);
            }
            CompoundCommand::CaseClause(clause) => {
                self.inspect_argument_word(&clause.value, depth + 1);
                for case in &clause.cases {
                    for pattern in &case.patterns {
                        self.inspect_argument_word(pattern, depth + 1);
                    }
                    if let Some(list) = &case.cmd {
                        self.visit_list(list, depth + 1);
                    }
                }
            }
            CompoundCommand::IfClause(clause) => {
                self.visit_list(&clause.condition, depth + 1);
                self.visit_list(&clause.then, depth + 1);
                if let Some(elses) = &clause.elses {
                    for else_clause in elses {
                        if let Some(condition) = &else_clause.condition {
                            self.visit_list(condition, depth + 1);
                        }
                        self.visit_list(&else_clause.body, depth + 1);
                    }
                }
            }
            CompoundCommand::WhileClause(clause) | CompoundCommand::UntilClause(clause) => {
                self.visit_list(&clause.0, depth + 1);
                self.visit_list(&clause.1.list, depth + 1);
            }
            CompoundCommand::Coprocess(coprocess) => {
                self.flag(RiskTier::Mutating, "Coprocess execution requires approval");
                self.visit_command(&coprocess.body, depth + 1);
            }
            CompoundCommand::Arithmetic(command) => {
                self.flag(RiskTier::Mutating, "Shell arithmetic may mutate dynamic variables");
                self.inspect_arithmetic(&command.expr.value, depth + 1);
            }
            CompoundCommand::ArithmeticForClause(clause) => {
                self.flag(RiskTier::Mutating, "Shell arithmetic may mutate dynamic variables");
                for expression in [&clause.initializer, &clause.condition, &clause.updater]
                    .into_iter()
                    .flatten()
                {
                    self.inspect_arithmetic(&expression.value, depth + 1);
                }
                self.visit_list(&clause.body.list, depth + 1);
            }
        }
    }

    fn visit_extended_test(&mut self, expression: &ExtendedTestExpr, depth: usize) {
        match expression {
            ExtendedTestExpr::And(left, right) | ExtendedTestExpr::Or(left, right) => {
                self.visit_extended_test(left, depth + 1);
                self.visit_extended_test(right, depth + 1);
            }
            ExtendedTestExpr::Not(nested) | ExtendedTestExpr::Parenthesized(nested) => {
                self.visit_extended_test(nested, depth + 1);
            }
            ExtendedTestExpr::UnaryTest(_, word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            ExtendedTestExpr::BinaryTest(_, left, right) => {
                self.inspect_argument_word(left, depth + 1);
                self.inspect_argument_word(right, depth + 1);
            }
        }
    }

    fn visit_simple(&mut self, command: &SimpleCommand, depth: usize) {
        if !self.check_depth(depth) {
            return;
        }
        let mut arguments = Vec::new();
        if let Some(prefix) = &command.prefix {
            self.visit_items(&prefix.0, (&mut arguments, depth + 1));
        }

        let Some(command_word) = &command.word_or_name else {
            self.flag(
                RiskTier::Mutating,
                "Shell assignment without a command mutates shell state",
            );
            return;
        };
        let command_name = self.inspect_command_word(command_word, depth + 1);

        if let Some(suffix) = &command.suffix {
            self.visit_items(&suffix.0, (&mut arguments, depth + 1));
        }

        let Some(command_name) = command_name else {
            self.flag(
                RiskTier::Mutating,
                "Dynamic expansion in command position cannot be resolved safely",
            );
            return;
        };
        let base = Path::new(&command_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&command_name)
            .to_ascii_lowercase();
        self.commands.push(base.clone());
        self.classify_invocation(&base, &arguments);
    }

    fn visit_items(&mut self, items: &[CommandPrefixOrSuffixItem], context: (&mut Vec<String>, usize)) {
        let (arguments, depth) = context;
        for item in items {
            match item {
                CommandPrefixOrSuffixItem::Word(word) => {
                    arguments.push(self.inspect_argument_word(word, depth + 1).unwrap_or_default());
                }
                CommandPrefixOrSuffixItem::AssignmentWord(_, word) => {
                    self.inspect_argument_word(word, depth + 1);
                }
                CommandPrefixOrSuffixItem::IoRedirect(redirect) => self.visit_redirect(redirect, depth + 1),
                CommandPrefixOrSuffixItem::ProcessSubstitution(_, subshell) => {
                    self.visit_list(&subshell.list, depth + 1);
                }
            }
        }
    }

    fn inspect_argument_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (false, depth))
    }

    fn inspect_command_word(&mut self, word: &Word, depth: usize) -> Option<String> {
        self.inspect_word(word, (true, depth))
    }

    fn inspect_word(&mut self, word: &Word, context: (bool, usize)) -> Option<String> {
        let (command_position, depth) = context;
        if !self.check_depth(depth) {
            return None;
        }
        let Ok(pieces) = word::parse(&word.value, &self.options) else {
            self.flag(RiskTier::Mutating, "Shell word expansion could not be parsed safely");
            return None;
        };
        self.inspect_pieces(&pieces, (command_position, depth + 1))
    }

    fn inspect_pieces(&mut self, pieces: &[WordPieceWithSource], context: (bool, usize)) -> Option<String> {
        let (command_position, depth) = context;
        let mut literal = String::new();
        let mut is_literal = true;
        for piece in pieces {
            match &piece.piece {
                WordPiece::Text(text)
                | WordPiece::SingleQuotedText(text)
                | WordPiece::AnsiCQuotedText(text)
                | WordPiece::EscapeSequence(text) => literal.push_str(text),
                WordPiece::DoubleQuotedSequence(nested) | WordPiece::GettextDoubleQuotedSequence(nested) => {
                    if let Some(text) = self.inspect_pieces(nested, (command_position, depth + 1)) {
                        literal.push_str(&text);
                    } else {
                        is_literal = false;
                    }
                }
                WordPiece::CommandSubstitution(source) | WordPiece::BackquotedCommandSubstitution(source) => {
                    self.visit_substitution(source, depth + 1);
                    is_literal = false;
                }
                WordPiece::ArithmeticExpression(expression) => {
                    self.inspect_arithmetic(&expression.value, depth + 1);
                    is_literal = false;
                    if command_position {
                        self.flag(
                            RiskTier::Mutating,
                            "Dynamic expansion in command position cannot be resolved safely",
                        );
                    }
                }
                WordPiece::ParameterExpansion(_) | WordPiece::TildeExpansion(_) => {
                    is_literal = false;
                    if command_position {
                        self.flag(
                            RiskTier::Mutating,
                            "Dynamic expansion in command position cannot be resolved safely",
                        );
                    }
                }
            }
        }
        is_literal.then_some(literal)
    }

    fn inspect_arithmetic(&mut self, source: &str, depth: usize) {
        let Ok(pieces) = word::parse(source, &self.options) else {
            self.flag(
                RiskTier::Mutating,
                "Shell arithmetic expansion could not be parsed safely",
            );
            return;
        };
        self.inspect_pieces(&pieces, (false, depth + 1));
    }

    fn visit_substitution(&mut self, source: &str, depth: usize) {
        if !self.check_depth(depth) {
            return;
        }
        let mut parser = Parser::new(Cursor::new(source.as_bytes()), &self.options);
        match parser.parse_program() {
            Ok(program) => {
                for command in &program.complete_commands {
                    self.visit_list(command, depth + 1);
                }
            }
            Err(_) => self.flag(RiskTier::Mutating, "Command substitution could not be parsed safely"),
        }
    }

    fn visit_redirects(&mut self, redirects: &RedirectList, depth: usize) {
        for redirect in &redirects.0 {
            self.visit_redirect(redirect, depth + 1);
        }
    }

    fn visit_redirect(&mut self, redirect: &IoRedirect, depth: usize) {
        match redirect {
            IoRedirect::File(_, kind, target) => {
                self.visit_redirect_target(target, depth + 1);
                if !matches!(kind, IoFileRedirectKind::Read | IoFileRedirectKind::DuplicateInput) {
                    self.classify_output_redirect(kind, target);
                }
            }
            IoRedirect::HereDocument(_, document) => {
                if document.requires_expansion {
                    self.inspect_argument_word(&document.doc, depth + 1);
                }
            }
            IoRedirect::HereString(_, word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            IoRedirect::OutputAndError(target, _) => {
                self.inspect_argument_word(target, depth + 1);
                self.classify_output_path(&target.value);
            }
        }
    }

    fn visit_redirect_target(&mut self, target: &IoFileRedirectTarget, depth: usize) {
        match target {
            IoFileRedirectTarget::Filename(word) => {
                self.inspect_argument_word(word, depth + 1);
            }
            IoFileRedirectTarget::Duplicate(word) => {
                if self.inspect_argument_word(word, depth + 1).is_none() {
                    self.flag(
                        RiskTier::Mutating,
                        "Dynamic file descriptor redirection cannot be resolved safely",
                    );
                }
            }
            IoFileRedirectTarget::ProcessSubstitution(_, subshell) => self.visit_list(&subshell.list, depth + 1),
            IoFileRedirectTarget::Fd(_) => {}
        }
    }

    fn classify_output_redirect(&mut self, kind: &IoFileRedirectKind, target: &IoFileRedirectTarget) {
        match target {
            IoFileRedirectTarget::Fd(_) | IoFileRedirectTarget::Duplicate(_) => {}
            IoFileRedirectTarget::Filename(word) => self.classify_output_path(&word.value),
            IoFileRedirectTarget::ProcessSubstitution(_, _) => {}
        }
        if matches!(kind, IoFileRedirectKind::ReadAndWrite) {
            self.flag(RiskTier::Mutating, "Read/write redirection may modify its target");
        }
    }

    fn classify_output_path(&mut self, raw: &str) {
        let path = unquote(raw);
        if matches!(path.as_str(), "/dev/null" | "/dev/stdout" | "/dev/stderr") {
            return;
        }
        if path.starts_with("/dev/") || path.starts_with("/etc/") {
            self.flag(
                RiskTier::HighRisk,
                format!("HIGH RISK: output redirection targets sensitive path {path}"),
            );
        } else {
            self.flag(
                RiskTier::Mutating,
                format!("Writes output through file redirection to {path}"),
            );
        }
    }

    fn classify_invocation(&mut self, command: &str, arguments: &[String]) {
        if is_high_risk_command(command, arguments) {
            self.flag(
                RiskTier::HighRisk,
                format!("HIGH RISK: destructive or elevated command: {command}"),
            );
            return;
        }
        match command {
            "eval" | "exec" | "source" | "." => self.flag(
                RiskTier::Mutating,
                format!("Dynamic shell execution requires approval: {command}"),
            ),
            "cd" if matches!(arguments, [path] if matches!(path.as_str(), "." | "./")) => {}
            "cd" => self.flag(
                RiskTier::Mutating,
                "Changing the shell working directory requires approval",
            ),
            "find" if has_any(arguments, &["-delete", "-exec", "-execdir", "-ok", "-okdir"]) => self.flag(
                RiskTier::Mutating,
                "find invocation can execute commands or delete files",
            ),
            "find" => {}
            "git" if is_read_only_git(arguments) => {}
            "git" => self.flag(RiskTier::Mutating, "Git invocation may modify repository state"),
            "cargo" if is_read_only_cargo(arguments) => {}
            "cargo" => self.flag(
                RiskTier::Mutating,
                "Cargo invocation may execute or modify project state",
            ),
            "tee" if arguments.is_empty() || arguments.iter().all(|arg| arg == "/dev/null") => {}
            command if READ_ONLY_COMMANDS.contains(&command) => {}
            command => self.flag(
                RiskTier::Mutating,
                format!("Command is not on the verified read-only list: {command}"),
            ),
        }
    }
}

const READ_ONLY_COMMANDS: &[&str] = &[
    "[", "test", "true", "false", "ls", "dir", "pwd", "cat", "head", "tail", "grep", "rg", "ag", "fd", "tree", "wc",
    "stat", "file", "which", "whereis", "type", "echo", "printf", "printenv", "uname", "sw_vers", "df", "du", "ps",
    "whoami", "uptime", "diff", "cmp", "sort", "uniq", "cut", "tr", "basename", "dirname", "realpath", "readlink",
];

fn is_high_risk_command(command: &str, arguments: &[String]) -> bool {
    match command {
        "sudo" | "doas" | "su" | "mkfs" | "fdisk" | "shred" => true,
        "dd" => true,
        "rm" => {
            has_short_flag(arguments, 'r')
                || has_short_flag(arguments, 'R')
                || has_short_flag(arguments, 'f')
                || has_any(arguments, &["--recursive", "--force"])
        }
        "unlink" => arguments.iter().any(|arg| contains_glob(arg)),
        "git" => is_high_risk_git(arguments),
        _ => false,
    }
}

fn is_high_risk_git(arguments: &[String]) -> bool {
    match arguments.first().map(String::as_str) {
        Some("reset") => has_any(arguments, &["--hard"]),
        Some("clean") => has_short_flag(arguments, 'f') || has_any(arguments, &["--force"]),
        Some("push") => has_any(arguments, &["--force", "--force-with-lease"]) || has_short_flag(arguments, 'f'),
        Some("checkout") => arguments.windows(2).any(|args| args == ["--", "."]),
        Some("restore") => arguments.iter().any(|arg| arg == "."),
        _ => false,
    }
}

fn is_read_only_git(arguments: &[String]) -> bool {
    let Some(subcommand) = arguments.first().map(String::as_str) else {
        return false;
    };
    match subcommand {
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files" | "describe" | "--version" => true,
        "branch" => arguments[1..]
            .iter()
            .all(|arg| matches!(arg.as_str(), "-a" | "-r" | "-v" | "-vv" | "--list" | "--show-current")),
        "tag" => arguments[1..].iter().all(|arg| matches!(arg.as_str(), "-l" | "--list")),
        "remote" => {
            arguments.len() == 1
                || matches!(&arguments[1..], [arg] if arg == "-v")
                || matches!(&arguments[1..], [action, _] if action == "get-url")
        }
        "config" => {
            arguments.len() == 2
                || arguments.get(1).is_some_and(|arg| {
                    matches!(
                        arg.as_str(),
                        "--get" | "--get-all" | "--get-regexp" | "--list" | "-l" | "--show-origin"
                    )
                })
        }
        _ => false,
    }
}

fn is_read_only_cargo(arguments: &[String]) -> bool {
    arguments.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "check" | "clippy" | "test" | "tree" | "metadata" | "--version" | "version"
        )
    })
}

fn has_any(arguments: &[String], values: &[&str]) -> bool {
    arguments.iter().any(|arg| values.contains(&arg.as_str()))
}

fn has_short_flag(arguments: &[String], flag: char) -> bool {
    arguments
        .iter()
        .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(flag))
}

fn contains_glob(value: &str) -> bool {
    value.contains(['*', '?', '['])
}

fn unquote(value: &str) -> String {
    value.trim_matches(['\'', '"']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_tiers_are_ordered() {
        assert!(RiskTier::ReadOnly < RiskTier::Mutating);
        assert!(RiskTier::Mutating < RiskTier::HighRisk);
    }

    #[test]
    fn classifies_shell_structures() {
        let cases = [
            ("ls -la", RiskTier::ReadOnly),
            ("cat Cargo.toml", RiskTier::ReadOnly),
            ("head -n 2 Cargo.toml", RiskTier::ReadOnly),
            ("tail -n 2 Cargo.toml", RiskTier::ReadOnly),
            ("pwd", RiskTier::ReadOnly),
            ("rg 'fn main' src", RiskTier::ReadOnly),
            ("grep -R needle src", RiskTier::ReadOnly),
            ("find . -name '*.rs'", RiskTier::ReadOnly),
            ("git status", RiskTier::ReadOnly),
            ("git diff --stat", RiskTier::ReadOnly),
            ("git log -1", RiskTier::ReadOnly),
            ("git show HEAD:file", RiskTier::ReadOnly),
            ("git rev-parse HEAD", RiskTier::ReadOnly),
            ("git ls-files", RiskTier::ReadOnly),
            ("git describe --tags", RiskTier::ReadOnly),
            ("git branch --show-current", RiskTier::ReadOnly),
            ("git branch -a", RiskTier::ReadOnly),
            ("git tag --list", RiskTier::ReadOnly),
            ("git remote -v", RiskTier::ReadOnly),
            ("git remote get-url origin", RiskTier::ReadOnly),
            ("git config --get user.name", RiskTier::ReadOnly),
            ("cargo check", RiskTier::ReadOnly),
            ("cargo clippy --all-targets", RiskTier::ReadOnly),
            ("cargo test --all-targets", RiskTier::ReadOnly),
            ("cargo tree", RiskTier::ReadOnly),
            ("cargo metadata", RiskTier::ReadOnly),
            ("cat Cargo.toml | rg rig", RiskTier::ReadOnly),
            ("git status && cargo check", RiskTier::ReadOnly),
            ("false || git status", RiskTier::ReadOnly),
            ("(git status && cargo check) | rg error", RiskTier::ReadOnly),
            ("{ git status; cargo check; }", RiskTier::ReadOnly),
            ("cat < Cargo.toml", RiskTier::ReadOnly),
            ("cd .", RiskTier::ReadOnly),
            ("cd ./ && git status", RiskTier::ReadOnly),
            ("git status 2>/dev/null", RiskTier::ReadOnly),
            ("echo test > /dev/stdout", RiskTier::ReadOnly),
            ("git status 2>&1", RiskTier::ReadOnly),
            ("echo \"Commit: $(git rev-parse HEAD)\"", RiskTier::ReadOnly),
            ("printf '%s' `git rev-parse HEAD`", RiskTier::ReadOnly),
            ("diff <(git show HEAD:file) <(cat file)", RiskTier::ReadOnly),
            ("RUST_LOG=debug cargo check", RiskTier::ReadOnly),
            (
                "if git status; then cargo check; else cargo test; fi",
                RiskTier::ReadOnly,
            ),
            ("for f in a b; do echo \"$f\"; done", RiskTier::ReadOnly),
            ("cat <<'EOF'\nhello\nEOF", RiskTier::ReadOnly),
            ("[[ $(git status) == '' ]]", RiskTier::ReadOnly),
            ("echo test > test.txt", RiskTier::Mutating),
            ("cat >> output.log", RiskTier::Mutating),
            ("cat <> state", RiskTier::Mutating),
            ("cat Cargo.toml | tee copy.toml", RiskTier::Mutating),
            ("touch marker", RiskTier::Mutating),
            ("cd ..", RiskTier::Mutating),
            ("cd subdir && cat secrets.txt", RiskTier::Mutating),
            ("git commit -m msg", RiskTier::Mutating),
            ("git checkout main", RiskTier::Mutating),
            ("git branch new", RiskTier::Mutating),
            ("git config user.name model", RiskTier::Mutating),
            ("cargo run", RiskTier::Mutating),
            ("npm install", RiskTier::Mutating),
            ("find . -delete", RiskTier::Mutating),
            ("find . -exec cat {} ';'", RiskTier::Mutating),
            ("eval \"$USER_INPUT\"", RiskTier::Mutating),
            ("exec sh", RiskTier::Mutating),
            ("source script.sh", RiskTier::Mutating),
            ("$CMD arg", RiskTier::Mutating),
            ("echo $(touch marker)", RiskTier::Mutating),
            ("[[ $(touch marker) == x ]]", RiskTier::Mutating),
            ("for ((i=0; i<1; i++)); do rm -rf target; done", RiskTier::HighRisk),
            ("echo $(( $(rm -rf target) + 1 ))", RiskTier::HighRisk),
            ("printf `touch marker`", RiskTier::Mutating),
            ("foo() { touch marker; }; foo", RiskTier::Mutating),
            ("echo test > /etc/hosts", RiskTier::HighRisk),
            ("echo test > /dev/sda", RiskTier::HighRisk),
            ("rm -rf target", RiskTier::HighRisk),
            ("rm -f file", RiskTier::HighRisk),
            ("rm --recursive target", RiskTier::HighRisk),
            ("unlink '*.tmp'", RiskTier::HighRisk),
            ("git reset --hard HEAD~1", RiskTier::HighRisk),
            ("git clean -fd", RiskTier::HighRisk),
            ("git push --force origin main", RiskTier::HighRisk),
            ("git checkout -- .", RiskTier::HighRisk),
            ("git restore .", RiskTier::HighRisk),
            ("sudo apt install x", RiskTier::HighRisk),
            ("doas reboot", RiskTier::HighRisk),
            ("mkfs /dev/sda", RiskTier::HighRisk),
            ("dd if=/dev/zero of=/dev/sda", RiskTier::HighRisk),
            ("shred file", RiskTier::HighRisk),
            ("echo 'unterminated", RiskTier::Mutating),
            ("if true; then echo x", RiskTier::Mutating),
            ("", RiskTier::Mutating),
        ];

        assert!(cases.len() >= 50);
        for (command, expected) in cases {
            let analysis = analyze_command_safety(command);
            assert_eq!(analysis.tier, expected, "{command}: {:?}", analysis.reasons);
            if expected != RiskTier::ReadOnly {
                assert!(!analysis.reasons.is_empty(), "{command}");
            }
        }
    }

    #[test]
    fn reports_nested_commands() {
        let analysis = analyze_command_safety("echo $(git status) | rg clean");
        assert_eq!(analysis.commands, ["git", "echo", "rg"]);
    }

    #[test]
    fn standard_command_analysis_averages_under_one_millisecond() {
        let commands = [
            "git status && cargo check",
            "cat Cargo.toml | rg brush",
            "echo \"Commit: $(git rev-parse HEAD)\"",
            "echo test > output.txt",
        ];
        for command in commands {
            analyze_command_safety(command);
        }

        let iterations = 500;
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            for command in commands {
                std::hint::black_box(analyze_command_safety(command));
            }
        }
        let average = started.elapsed() / (iterations * commands.len()) as u32;

        assert!(average < std::time::Duration::from_millis(1), "average: {average:?}");
    }
}
