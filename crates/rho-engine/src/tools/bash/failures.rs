//! Failing tests found in the full output of a clipped command, so the model sees which tests
//! failed and why without reading the log. Recognizes libtest, cargo-nextest, pytest, Go, Jest and Vitest.

use std::sync::LazyLock;

const MAX_FAILURES: usize = 20;
const MAX_TRACKED: usize = 5000;
const MAX_DETAILS: usize = 5;
const MAX_LINES: usize = 60;
const SECTION_END_PREFIXES: [&str; 7] = [
    "failures:",
    "test result:",
    "===",
    "Test Suites:",
    "Test Files",
    "ok  ",
    "ok\t",
];
const SECTION_END_WORDS: [&str; 2] = ["FAIL", "PASS"];
/// A runner's own totals reporting failures: libtest, nextest, pytest, Jest and Vitest print `N failed`,
/// Go prints a bare `FAIL` line.
static FAILED_TOTALS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?m)^test result: FAILED|\b[1-9]\d* failed\b|^FAIL(\s|$)").expect("valid totals regex")
});
static DURATION: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s+\(?\d+(?:\.\d+)?\s?m?s\)?$").expect("valid duration regex"));

#[derive(Debug, Clone, Copy)]
enum Details {
    Panic,
    PytestError,
    GoLog,
    JestMessage,
}

impl Details {
    fn keeps(self, line: &str, continuing: bool) -> bool {
        let trimmed = line.trim();
        match self {
            Self::Panic => {
                line.contains("panicked at") || (continuing && !trimmed.is_empty() && !trimmed.starts_with("note:"))
            }
            Self::PytestError => line.starts_with("E "),
            Self::GoLog => is_go_log(trimmed) || (continuing && line.starts_with([' ', '\t']) && !trimmed.is_empty()),
            Self::JestMessage => {
                trimmed.starts_with(|c: char| c.is_ascii_alphabetic() || c == '-' || c == '+')
                    && !trimmed.starts_with("at ")
            }
        }
    }
}

fn is_go_log(trimmed: &str) -> bool {
    trimmed
        .split_once(':')
        .is_some_and(|(file, _)| file.ends_with("_test.go"))
}

struct Header<'a> {
    name: String,
    failed: bool,
    details: Option<Details>,
    message: Option<&'a str>,
}

impl Header<'_> {
    fn failed(name: &str) -> Self {
        Self {
            name: name.to_string(),
            failed: true,
            details: None,
            message: None,
        }
    }

    fn block(name: &str, failed: bool, details: Details) -> Self {
        Self {
            name: name.to_string(),
            failed,
            details: Some(details),
            message: None,
        }
    }
}

const HEADERS: [fn(&str) -> Option<Header<'_>>; 10] = [
    libtest_result,
    libtest_block,
    nextest_result,
    nextest_block,
    pytest_block,
    pytest_summary,
    go_failure,
    jest_mark,
    jest_block,
    vitest_block,
];

fn libtest_result(line: &str) -> Option<Header<'_>> {
    let name = line.strip_prefix("test ")?.strip_suffix(" ... FAILED")?;
    Some(Header::failed(name))
}

fn libtest_block(line: &str) -> Option<Header<'_>> {
    let name = line.strip_prefix("---- ")?.strip_suffix(" stdout ----")?;
    Some(Header::block(name, false, Details::Panic))
}

fn nextest_result(line: &str) -> Option<Header<'_>> {
    let (_, name) = line.trim_start().strip_prefix("FAIL [")?.split_once("] ")?;
    let name = match name.split_once(") ") {
        Some((counter, rest)) if counter.starts_with('(') => rest,
        _ => name,
    };
    Some(Header::failed(name.trim()))
}

fn nextest_block(line: &str) -> Option<Header<'_>> {
    let rest = line
        .strip_prefix("--- STDOUT:")
        .or_else(|| line.strip_prefix("--- STDERR:"))?;
    Some(Header::block(rest.strip_suffix("---")?.trim(), false, Details::Panic))
}

fn pytest_block(line: &str) -> Option<Header<'_>> {
    if !line.starts_with("___") || !line.ends_with("___") {
        return None;
    }
    let name = line.trim_matches('_').trim();
    if name.is_empty() {
        return None;
    }
    let (path, params) = name.split_at(name.find('[').unwrap_or(name.len()));
    let node = format!("{}{params}", path.replace('.', "::"));
    Some(Header::block(&node, true, Details::PytestError))
}

fn pytest_summary(line: &str) -> Option<Header<'_>> {
    let rest = line.strip_prefix("FAILED ")?;
    let (name, message) = match rest.split_once(" - ") {
        Some((name, message)) => (name, Some(message)),
        None => (rest, None),
    };
    if !name.contains("::") && !name.contains(".py") {
        return None;
    }
    Some(Header {
        message,
        ..Header::failed(name.trim())
    })
}

fn go_failure(line: &str) -> Option<Header<'_>> {
    let rest = line.trim_start().strip_prefix("--- FAIL: ")?;
    let name = rest.split(" (").next().unwrap_or(rest);
    Some(Header::block(name.trim(), true, Details::GoLog))
}

fn jest_mark(line: &str) -> Option<Header<'_>> {
    let rest = line.trim_start().strip_prefix(['\u{2715}', '\u{d7}'])?;
    Some(Header::failed(&DURATION.replace(rest.trim(), "")))
}

fn jest_block(line: &str) -> Option<Header<'_>> {
    let rest = line.trim_start().strip_prefix('\u{25cf}')?;
    Some(Header::block(rest.trim(), true, Details::JestMessage))
}

fn vitest_block(line: &str) -> Option<Header<'_>> {
    let name = line.trim_start().strip_prefix("FAIL ")?.trim();
    name.contains(" > ")
        .then(|| Header::block(name, true, Details::JestMessage))
}

/// A header's name and an earlier one name the same test when one ends with the other after a
/// path or suite separator, e.g. `Suite > test` and `test`, or `crate tests::it` and `tests::it`.
fn same_test(a: &str, b: &str) -> bool {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    long.strip_suffix(short)
        .is_some_and(|head| head.is_empty() || head.ends_with("::") || head.ends_with(' '))
}

/// A runner's section markers end the open block; a detail line merely starting with the same
/// letters (`FAILED to connect`, `PASSWORD missing`) does not.
fn ends_section(line: &str) -> bool {
    let line = line.strip_prefix(' ').unwrap_or(line);
    SECTION_END_PREFIXES.iter().any(|end| line.starts_with(end))
        || SECTION_END_WORDS.iter().any(|word| {
            line.strip_prefix(word)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        })
}

struct Failure {
    name: String,
    failed: bool,
    details: Vec<String>,
}

impl Failure {
    fn add_detail(&mut self, line: &str) {
        if self.details.len() < MAX_DETAILS {
            let line = crate::tools::truncate::truncate_line(line.trim());
            self.details.push(line.text.into_owned());
        }
    }
}

#[derive(Default)]
struct Scan {
    failures: Vec<Failure>,
    open: Option<(usize, Details)>,
    /// The failure whose block header came last; breaks ties between same-named tests of different crates.
    scope: Option<usize>,
    continuing: bool,
}

impl Scan {
    fn read(&mut self, line: &str) {
        if let Some(header) = HEADERS.iter().find_map(|parse| parse(line)) {
            self.start(header);
        } else if ends_section(line) {
            self.open = None;
        } else if let Some((index, details)) = self.open {
            self.continuing = details.keeps(line, self.continuing);
            if self.continuing {
                self.failures[index].add_detail(line);
            }
        }
    }

    fn start(&mut self, header: Header<'_>) {
        let Some(index) = self.entry(header.name) else {
            self.open = None;
            return;
        };
        let failure = &mut self.failures[index];
        failure.failed |= header.failed;
        if let Some(message) = header.message
            && failure.details.is_empty()
        {
            failure.add_detail(message);
        }
        self.open = header.details.map(|details| (index, details));
        if header.details.is_some() {
            self.scope = Some(index);
        }
        self.continuing = false;
    }

    fn find(&self, name: &str) -> Option<usize> {
        let candidates: Vec<usize> = (0..self.failures.len())
            .filter(|&i| same_test(&self.failures[i].name, name))
            .collect();
        candidates
            .iter()
            .copied()
            .find(|&i| self.failures[i].name == name)
            .or_else(|| self.scope.filter(|scope| candidates.contains(scope)))
            .or_else(|| candidates.first().copied())
    }

    /// The index of the failure named `name`, adding it; `None` once `MAX_TRACKED` tests are tracked,
    /// which keeps the same-test lookup bounded on logs with huge numbers of test headers.
    fn entry(&mut self, name: String) -> Option<usize> {
        if let Some(index) = self.find(&name) {
            if name.len() > self.failures[index].name.len() {
                self.failures[index].name = name;
            }
            return Some(index);
        }
        if self.failures.len() >= MAX_TRACKED {
            return None;
        }
        self.failures.push(Failure {
            name,
            failed: false,
            details: Vec::new(),
        });
        Some(self.failures.len() - 1)
    }
}

/// Like [`summarize_test_failures`] for a command that exited 0, whose output merely quoting test
/// results (`cat`, `git diff`) is not a failing run: needs the runner's own totals to report failures.
pub(crate) fn summarize_reported_failures(output: &str) -> Option<String> {
    let output = super::sanitize::strip_ansi(output);
    if FAILED_TOTALS.is_match(&output) {
        summarize_test_failures(&output)
    } else {
        None
    }
}

/// The failing tests in `output` with their key failure lines, or `None` when none were found.
pub(crate) fn summarize_test_failures(output: &str) -> Option<String> {
    let output = super::sanitize::strip_ansi(output);
    let mut scan = Scan::default();
    output.lines().for_each(|line| scan.read(line));
    let failed: Vec<&Failure> = scan.failures.iter().filter(|f| f.failed).collect();
    if failed.is_empty() {
        return None;
    }
    let plural = if failed.len() == 1 { "" } else { "s" };
    let mut lines = vec![format!("[{} failing test{plural}; details from log]", failed.len())];
    let mut shown = 0;
    for failure in failed.iter().take(MAX_FAILURES) {
        if lines.len() + 1 + failure.details.len() > MAX_LINES - 1 {
            break;
        }
        lines.push(format!("- {}", failure.name));
        lines.extend(failure.details.iter().map(|detail| format!("    {detail}")));
        shown += 1;
    }
    if shown < failed.len() {
        lines.push(format!("[{} more failing tests in the log]", failed.len() - shown));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests;
