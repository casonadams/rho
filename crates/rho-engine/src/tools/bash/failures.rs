//! Failing tests found in the full output of a clipped command, so the model sees which tests
//! failed and why without reading the log. Recognizes libtest, cargo-nextest, pytest, Go, Jest and Vitest.

use std::sync::LazyLock;

const MAX_FAILURES: usize = 20;
const MAX_TRACKED: usize = 5000;
const MAX_DETAILS: usize = 5;
const MAX_LINES: usize = 60;
const SECTION_ENDS: [&str; 9] = [
    "failures:",
    "test result:",
    "===",
    "FAIL",
    "PASS",
    "ok ",
    "ok\t",
    "Test Suites:",
    "Test Files",
];
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

fn ends_section(line: &str) -> bool {
    let line = line.strip_prefix(' ').unwrap_or(line);
    SECTION_ENDS.iter().any(|end| line.starts_with(end))
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
        self.continuing = false;
    }

    /// The index of the failure named `name`, adding it; `None` once `MAX_TRACKED` tests are tracked,
    /// which keeps the same-test lookup bounded on logs with huge numbers of test headers.
    fn entry(&mut self, name: String) -> Option<usize> {
        if let Some(index) = self.failures.iter().position(|f| same_test(&f.name, &name)) {
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

/// The failing tests in `output` with their key failure lines, or `None` when none were found.
pub fn summarize_test_failures(output: &str) -> Option<String> {
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
mod tests {
    use super::*;

    const LIBTEST: &str = "\
running 3 tests
test math::adds ... ok
test math::subtracts ... FAILED
test io::reads ... FAILED

failures:

---- math::subtracts stdout ----
debug print
thread 'math::subtracts' panicked at src/math.rs:12:9:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- io::reads stdout ----
thread 'io::reads' panicked at src/io.rs:4:5:
missing file
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    math::subtracts
    io::reads

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";

    const NEXTEST: &str = "\
    Starting 3 tests across 1 binary
        PASS [   0.003s] demo tests::adds
        FAIL [   0.004s] demo tests::subtracts
--- STDOUT:              demo tests::subtracts ---

running 1 test
test tests::subtracts ... FAILED

failures:

failures:
    tests::subtracts

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2 filtered out

--- STDERR:              demo tests::subtracts ---
thread 'tests::subtracts' panicked at src/lib.rs:9:9:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

        PASS [   0.002s] demo tests::multiplies
------------
     Summary [   0.005s] 3 tests run: 2 passed, 1 failed, 0 skipped
        FAIL [   0.004s] demo tests::subtracts
error: test run failed";

    const PYTEST: &str = "\
============================= test session starts ==============================
collected 4 items

tests/test_math.py .F                                                    [ 50%]
tests/test_cls.py F                                                      [ 75%]
tests/test_misc.py F                                                     [100%]

=================================== FAILURES ===================================
________________________________ test_subtract _________________________________

    def test_subtract():
>       assert subtract(3, 1) == 3
E       assert 2 == 3
E        +  where 2 = subtract(3, 1)

tests/test_math.py:8: AssertionError
______________________________ TestUser.test_name ______________________________

self = <test_cls.TestUser object at 0x1>

    def test_name(self):
>       raise ValueError(\"bad name\")
E       ValueError: bad name

tests/test_cls.py:5: ValueError
=========================== short test summary info ============================
FAILED tests/test_math.py::test_subtract - assert 2 == 3
FAILED tests/test_cls.py::TestUser::test_name - ValueError: bad name
FAILED tests/test_misc.py::test_other - TimeoutError
========================= 3 failed, 1 passed in 0.03s ==========================";

    const GO: &str = "\
=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestSub
--- FAIL: TestSub (0.00s)
    math_test.go:14: Sub(3, 1) = 4; want 2
        extra context
--- FAIL: TestTable (0.00s)
    --- FAIL: TestTable/negative (0.00s)
        table_test.go:22: got -1, want 1
FAIL
FAIL\texample.com/math\t0.004s
FAIL";

    const JEST: &str = " FAIL  src/sum.test.js
  Math
    \u{2713} adds (2 ms)
    \u{2715} subtracts (3 ms)

  \u{25cf} Math \u{203a} subtracts

    expect(received).toBe(expected) // Object.is equality

    Expected: 2
    Received: 4

      3 | test('subtracts', () => {
    > 4 |   expect(sub(3, 1)).toBe(2);
        |                     ^
      5 | });

      at Object.<anonymous> (src/sum.test.js:4:21)

Test Suites: 1 failed, 1 total
Tests:       1 failed, 1 passed, 2 total";

    const VITEST: &str = " \u{276f} src/sum.test.ts (2 tests | 1 failed) 5ms
   \u{d7} math > subtracts 3ms

\u{23af}\u{23af}\u{23af} Failed Tests 1 \u{23af}\u{23af}\u{23af}

 FAIL  src/sum.test.ts > math > subtracts
AssertionError: expected 4 to be 2 // Object.is equality

- Expected
+ Received

- 2
+ 4

 \u{276f} src/sum.test.ts:6:21
      4|   it('subtracts', () => {
      5|     expect(sub(3, 1)).toBe(2)

 Test Files  1 failed (1)";

    #[test]
    fn each_framework_reports_its_failing_tests_with_their_key_lines() {
        let cases = [
            (
                "libtest",
                LIBTEST,
                "[2 failing tests; details from log]
- math::subtracts
    thread 'math::subtracts' panicked at src/math.rs:12:9:
    assertion `left == right` failed
    left: 1
    right: 2
- io::reads
    thread 'io::reads' panicked at src/io.rs:4:5:
    missing file",
            ),
            (
                "nextest",
                NEXTEST,
                "[1 failing test; details from log]
- demo tests::subtracts
    thread 'tests::subtracts' panicked at src/lib.rs:9:9:
    assertion `left == right` failed
    left: 1
    right: 2",
            ),
            (
                "pytest",
                PYTEST,
                "[3 failing tests; details from log]
- tests/test_math.py::test_subtract
    E       assert 2 == 3
    E        +  where 2 = subtract(3, 1)
- tests/test_cls.py::TestUser::test_name
    E       ValueError: bad name
- tests/test_misc.py::test_other
    TimeoutError",
            ),
            (
                "go",
                GO,
                "[3 failing tests; details from log]
- TestSub
    math_test.go:14: Sub(3, 1) = 4; want 2
    extra context
- TestTable
- TestTable/negative
    table_test.go:22: got -1, want 1",
            ),
            (
                "jest",
                JEST,
                "[1 failing test; details from log]
- Math \u{203a} subtracts
    expect(received).toBe(expected) // Object.is equality
    Expected: 2
    Received: 4",
            ),
            (
                "vitest",
                VITEST,
                "[1 failing test; details from log]
- src/sum.test.ts > math > subtracts
    AssertionError: expected 4 to be 2 // Object.is equality
    - Expected
    + Received
    - 2
    + 4",
            ),
        ];
        for (framework, output, expected) in cases {
            assert_eq!(
                summarize_test_failures(output).as_deref(),
                Some(expected),
                "{framework}"
            );
        }
    }

    #[test]
    fn output_without_failing_tests_has_no_summary() {
        for output in [
            "",
            "running 1 test\ntest math::adds ... ok\n\ntest result: ok. 1 passed",
            "--- PASS: TestAdd (0.00s)\nPASS\nok  \texample.com/math\t0.002s",
            "---- math::adds stdout ----\nprinted by a passing test with --show-output",
        ] {
            assert_eq!(summarize_test_failures(output), None, "{output}");
        }
    }

    #[test]
    fn long_failure_lists_are_capped_and_name_how_many_more_are_in_the_log() {
        let cases = [(25, 0, 20), (30, 3, 14)];
        for (count, details, shown) in cases {
            let output: String = (0..count)
                .map(|i| {
                    let detail: String = (0..details).map(|d| format!("    t_test.go:{d}: bad\n")).collect();
                    format!("--- FAIL: Test{i} (0.00s)\n{detail}")
                })
                .collect();
            let summary = summarize_test_failures(&output).unwrap();
            let lines: Vec<&str> = summary.lines().collect();
            assert!(lines.len() <= MAX_LINES, "{summary}");
            assert_eq!(lines[0], format!("[{count} failing tests; details from log]"));
            assert_eq!(lines.iter().filter(|l| l.starts_with("- ")).count(), shown);
            assert_eq!(
                lines.last(),
                Some(&format!("[{} more failing tests in the log]", count - shown).as_str())
            );
        }
    }
}
