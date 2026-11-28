use rho_engine::permission::bash::{Token, TokenKind, tokenize};
use std::collections::VecDeque;

const SCRATCH_PREFIXES: [&str; 7] = [
    "/dev/",
    "/tmp/",
    "/private/tmp/",
    "/var/folders/",
    "/private/var/folders/",
    "$TMPDIR/",
    "${TMPDIR}/",
];
const SCRIPT_PROGRAMS: [&str; 5] = ["python", "node", "ruby", "perl", "php"];
const SCRIPT_WRITE_MARKERS: [&str; 8] = [
    "'w')",
    "\"w\")",
    "'a')",
    "\"a\")",
    ".write(",
    "write_text(",
    "writeFileSync",
    "File.write",
];
const WRAPPERS: [&str; 7] = ["env", "sudo", "command", "nohup", "time", "exec", "nice"];
const WRAPPER_VALUE_FLAGS: [&str; 5] = ["-u", "-g", "-C", "-n", "-S"];
const XARGS_VALUE_FLAGS: [&str; 8] = ["-I", "-n", "-P", "-L", "-s", "-d", "-E", "-a"];
const SED_VALUE_FLAGS: &str = "efl";
const PERL_VALUE_FLAGS: &str = "MmIeEdDxFC";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum BashEdit {
    SedInPlace,
    PerlInPlace,
    Script,
    Redirect,
    Tee,
}

impl BashEdit {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SedInPlace => "sed -i",
            Self::PerlInPlace => "perl -i",
            Self::Script => "script writes a file",
            Self::Redirect => "shell redirect into a file",
            Self::Tee => "tee into a file",
        }
    }
}

fn is_scratch(path: &str) -> bool {
    SCRATCH_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

fn inside_quotes(prefix: &str) -> bool {
    prefix.matches('\'').count() % 2 == 1 || prefix.matches('"').count() % 2 == 1
}

fn heredoc_word(after_opener: &str) -> String {
    let after = after_opener.strip_prefix('-').unwrap_or(after_opener).trim_start();
    after
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '<' | '>' | ')'))
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect()
}

fn heredoc_delimiters(line: &str) -> Vec<String> {
    line.match_indices("<<")
        .filter(|(pos, _)| !line[pos + 2..].starts_with('<') && !inside_quotes(&line[..*pos]))
        .map(|(pos, _)| heredoc_word(&line[pos + 2..]))
        .filter(|word| !word.is_empty())
        .collect()
}

struct ShellScript {
    text: String,
    heredoc_bodies: VecDeque<String>,
}

fn split_heredocs(command: &str) -> ShellScript {
    let mut script = ShellScript {
        text: String::new(),
        heredoc_bodies: VecDeque::new(),
    };
    let mut pending: VecDeque<String> = VecDeque::new();
    let mut body = String::new();
    for line in command.split_inclusive('\n') {
        let Some(delimiter) = pending.front() else {
            script.text.push_str(line);
            pending.extend(heredoc_delimiters(line));
            continue;
        };
        if line.trim() == delimiter {
            pending.pop_front();
            script.heredoc_bodies.push_back(std::mem::take(&mut body));
        } else {
            body.push_str(line);
        }
    }
    if !pending.is_empty() {
        script.heredoc_bodies.push_back(body);
    }
    script
}

#[derive(Default)]
struct Segment {
    words: Vec<String>,
    redirects: Vec<(String, Option<String>)>,
    heredoc: String,
}

struct SegmentBuilder {
    segments: Vec<Segment>,
    in_comment: bool,
    heredoc_bodies: VecDeque<String>,
}

impl SegmentBuilder {
    fn push(&mut self, token: Token, next_word: impl FnOnce() -> Option<String>) {
        let starts_line = token.kind == TokenKind::Separator && token.raw == "\n";
        if self.in_comment && !starts_line {
            return;
        }
        if token.kind == TokenKind::Separator {
            self.in_comment = false;
            self.segments.push(Segment::default());
            return;
        }
        let Some(current) = self.segments.last_mut() else {
            return;
        };
        match token.kind {
            TokenKind::Word if token.raw.starts_with('#') => self.in_comment = true,
            TokenKind::Word => current.words.push(token.text),
            _ => {
                if token.raw.starts_with("<<") {
                    current
                        .heredoc
                        .push_str(&self.heredoc_bodies.pop_front().unwrap_or_default());
                }
                current.redirects.push((token.raw, next_word()));
            }
        }
    }
}

fn segments(command: &str) -> Vec<Segment> {
    let script = split_heredocs(command);
    let mut builder = SegmentBuilder {
        segments: vec![Segment::default()],
        in_comment: false,
        heredoc_bodies: script.heredoc_bodies,
    };
    let mut tokens = tokenize(&script.text).tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        let is_redirect = token.kind == TokenKind::Redirect;
        builder.push(token, || {
            is_redirect
                .then(|| tokens.next_if(|t| t.kind == TokenKind::Word))
                .flatten()
                .map(|t| t.text)
        });
    }
    builder.segments
}

fn program_name(word: &str) -> &str {
    let base = word.rsplit('/').next().unwrap_or(word);
    let versioned_python = base
        .strip_prefix("python")
        .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.'));
    match base {
        "gsed" => "sed",
        _ if versioned_python => "python",
        _ => base,
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn skip_options<'a, 'b>(words: &'a [&'b str], value_flags: &[&str]) -> &'a [&'b str] {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if is_assignment(word) {
            index += 1;
        } else if word.starts_with('-') {
            index += if value_flags.contains(word) { 2 } else { 1 };
        } else {
            break;
        }
    }
    &words[index.min(words.len())..]
}

fn effective_command<'a, 'b>(mut words: &'a [&'b str]) -> &'a [&'b str] {
    loop {
        let Some((first, rest)) = words.split_first() else {
            return words;
        };
        let program = program_name(first);
        words = if is_assignment(first) {
            rest
        } else if WRAPPERS.contains(&program) {
            skip_options(rest, &WRAPPER_VALUE_FLAGS)
        } else if program == "xargs" {
            skip_options(rest, &XARGS_VALUE_FLAGS)
        } else if let Some(exec) = (program == "find")
            .then(|| rest.iter().position(|w| matches!(*w, "-exec" | "-execdir" | "-ok")))
            .flatten()
        {
            &rest[exec + 1..]
        } else {
            return words;
        };
    }
}

/// Whether a short-option cluster such as `-ni` sets one of `flags` before an option that takes the rest as its value.
fn cluster_has(arg: &str, flags: &[char], value_flags: &str) -> bool {
    let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.starts_with('-')) else {
        return false;
    };
    for c in cluster.chars() {
        if flags.contains(&c) {
            return true;
        }
        if value_flags.contains(c) {
            return false;
        }
    }
    false
}

fn only_scratch_paths(script: &str) -> bool {
    let quoted = |quote: char| script.split(quote).skip(1).step_by(2).collect::<Vec<_>>();
    let paths: Vec<&str> = [quoted('\''), quoted('"')]
        .concat()
        .into_iter()
        .filter(|literal| literal.contains('/'))
        .collect();
    !paths.is_empty() && paths.iter().all(|path| is_scratch(path))
}

fn script_writes_file(script: &str) -> bool {
    let script = script.replace("stdout.write(", "").replace("stderr.write(", "");
    SCRIPT_WRITE_MARKERS.iter().any(|m| script.contains(m)) && !only_scratch_paths(&script)
}

fn command_edit(command: &[&str], heredoc: &str) -> Option<BashEdit> {
    let (program, args) = command.split_first()?;
    let sed_in_place =
        |a: &&str| *a == "--in-place" || a.starts_with("--in-place=") || cluster_has(a, &['i', 'I'], SED_VALUE_FLAGS);
    match program_name(program) {
        "sed" if args.iter().any(sed_in_place) => Some(BashEdit::SedInPlace),
        "perl" if args.iter().any(|a| cluster_has(a, &['i'], PERL_VALUE_FLAGS)) => Some(BashEdit::PerlInPlace),
        "tee" if args.iter().any(|a| !a.starts_with('-') && !is_scratch(a)) => Some(BashEdit::Tee),
        p if SCRIPT_PROGRAMS.contains(&p) && script_writes_file(&format!("{}\n{heredoc}", args.join(" "))) => {
            Some(BashEdit::Script)
        }
        _ => None,
    }
}

fn writes_file(operator: &str) -> bool {
    let operator = operator.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
    operator.starts_with('>') && !operator.contains(['<', '&'])
}

fn is_file_target(target: &str) -> bool {
    !target.starts_with('&') && !target.contains(['(', ')']) && !is_scratch(target)
}

fn segment_edit(segment: &Segment) -> Option<BashEdit> {
    let words: Vec<&str> = segment.words.iter().map(String::as_str).collect();
    if let Some(edit) = command_edit(effective_command(&words), &segment.heredoc) {
        return Some(edit);
    }
    let is_test = words.first().is_some_and(|w| *w == "[[");
    let redirects_into_file = segment
        .redirects
        .iter()
        .any(|(op, target)| writes_file(op) && target.as_deref().is_some_and(is_file_target));
    (redirects_into_file && !is_test).then_some(BashEdit::Redirect)
}

/// How a bash command changed files without the edit or write tools, if it did.
pub(super) fn classify_bash_edit(command: &str) -> Option<BashEdit> {
    segments(command).iter().find_map(segment_edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_file_edits_are_classified_and_scratch_writes_ignored() {
        let cases = [
            ("sed -i '' 's/a/b/' src/lib.rs", Some(BashEdit::SedInPlace)),
            ("sed -E -i.bak 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            ("sed -n 1,20p src/lib.rs", None),
            ("perl -pi -e 's/a/b/' x.rs", Some(BashEdit::PerlInPlace)),
            (
                "python3 - <<'EOF'\np='a.rs'\nopen(p,'w').write(s)\nEOF",
                Some(BashEdit::Script),
            ),
            ("python3 -c 'import json; print(json.load(open(\"a.json\")))'", None),
            ("cat > src/new.rs <<'EOF'\nfn main() {}\nEOF", Some(BashEdit::Redirect)),
            ("echo hi >> notes.md", Some(BashEdit::Redirect)),
            ("make all > /tmp/rho-make.log 2>&1", None),
            ("cargo test 2>&1 | grep FAIL", None),
            ("ls missing 2>/dev/null", None),
            ("echo x | tee out.txt", Some(BashEdit::Tee)),
            ("git status && cargo fmt --all", None),
            ("git commit -q -F - <<'EOF'\nfix: map Result<A> -> B > C\nEOF", None),
            ("git show HEAD:a.rs > a.rs", Some(BashEdit::Redirect)),
            ("rg -l foo | xargs sed -i 's/a/b/'", Some(BashEdit::SedInPlace)),
            ("rg -l foo | xargs -I {} sed -i 's/a/b/' {}", Some(BashEdit::SedInPlace)),
            (
                "find . -name '*.rs' -exec sed -i 's/a/b/' {} +",
                Some(BashEdit::SedInPlace),
            ),
            ("LC_ALL=C sed -i 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            (
                "sudo -u me gsed --in-place=.bak 's/a/b/' x.rs",
                Some(BashEdit::SedInPlace),
            ),
            ("sed -I '' 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            ("sed -e 's/i/x/' x.rs", None),
            (
                "env python3.12 -c \"open('a.rs','w').write('')\"",
                Some(BashEdit::Script),
            ),
            ("cargo build &> build.log", Some(BashEdit::Redirect)),
            ("cargo build &>> build.log", Some(BashEdit::Redirect)),
            ("perl -Mstrict -e 'print 1'", None),
            ("perl -MList::Util=min -Ilib -ne 'print' x", None),
            ("perl -pie 's/a/b/' x.rs", Some(BashEdit::PerlInPlace)),
            ("node -e \"process.stdout.write('x')\"", None),
            ("rg '\\.write\\(' src; python3 --version", None),
            ("python3 -c \"open('/tmp/x','w').write('')\"", None),
            ("echo $((1 > 0))", None),
            ("[[ $a > $b ]] && echo yes", None),
            ("cmd > >(tee /tmp/log)", None),
            ("echo hi # > x", None),
            ("echo $TMPDIR; echo x > $TMPDIR/out", None),
            (
                "cat > /tmp/p <<EOF\nbody > x\nEOF\nsed -i s/a/b/ src/x.rs",
                Some(BashEdit::SedInPlace),
            ),
            ("grep x <<< 'a > b'", None),
            ("git commit -m 'x << y'\nsed -i s/a/b/ f", Some(BashEdit::SedInPlace)),
        ];
        for (command, expected) in cases {
            assert_eq!(classify_bash_edit(command), expected, "{command}");
        }
    }
}
