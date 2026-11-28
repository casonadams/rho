use super::*;
use test_case::test_case;

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

#[test_case(
    LIBTEST,
    "[2 failing tests; details from log]
- math::subtracts
    thread 'math::subtracts' panicked at src/math.rs:12:9:
    assertion `left == right` failed
    left: 1
    right: 2
- io::reads
    thread 'io::reads' panicked at src/io.rs:4:5:
    missing file";
    "libtest"
)]
#[test_case(
    NEXTEST,
    "[1 failing test; details from log]
- demo tests::subtracts
    thread 'tests::subtracts' panicked at src/lib.rs:9:9:
    assertion `left == right` failed
    left: 1
    right: 2";
    "nextest"
)]
#[test_case(
    PYTEST,
    "[3 failing tests; details from log]
- tests/test_math.py::test_subtract
    E       assert 2 == 3
    E        +  where 2 = subtract(3, 1)
- tests/test_cls.py::TestUser::test_name
    E       ValueError: bad name
- tests/test_misc.py::test_other
    TimeoutError";
    "pytest"
)]
#[test_case(
    GO,
    "[3 failing tests; details from log]
- TestSub
    math_test.go:14: Sub(3, 1) = 4; want 2
    extra context
- TestTable
- TestTable/negative
    table_test.go:22: got -1, want 1";
    "go"
)]
#[test_case(
    JEST,
    "[1 failing test; details from log]
- Math \u{203a} subtracts
    expect(received).toBe(expected) // Object.is equality
    Expected: 2
    Received: 4";
    "jest"
)]
#[test_case(
    VITEST,
    "[1 failing test; details from log]
- src/sum.test.ts > math > subtracts
    AssertionError: expected 4 to be 2 // Object.is equality
    - Expected
    + Received
    - 2
    + 4";
    "vitest"
)]
fn each_framework_reports_its_failing_tests_with_their_key_lines(output: &str, expected: &str) {
    assert_eq!(summarize_test_failures(output).as_deref(), Some(expected));
}

#[test_case(""; "empty")]
#[test_case("running 1 test\ntest math::adds ... ok\n\ntest result: ok. 1 passed"; "libtest passing")]
#[test_case("--- PASS: TestAdd (0.00s)\nPASS\nok  \texample.com/math\t0.002s"; "go passing")]
#[test_case("---- math::adds stdout ----\nprinted by a passing test with --show-output"; "passing stdout section")]
fn output_without_failing_tests_has_no_summary(output: &str) {
    assert_eq!(summarize_test_failures(output), None);
}

#[test]
fn detail_lines_that_start_like_a_section_marker_stay_in_the_block() {
    let output = "test net::connects ... FAILED\n---- net::connects stdout ----\n\
                  thread 'net::connects' panicked at src/net.rs:3:5:\nFAILED to connect\nPASSWORD missing\n\
                  ok so far\n\nfailures:\n";

    let summary = summarize_test_failures(output).unwrap();

    for detail in ["FAILED to connect", "PASSWORD missing", "ok so far"] {
        assert!(summary.contains(&format!("    {detail}")), "{detail}: {summary}");
    }
}

#[test_case(25, 0, 20; "names only")]
#[test_case(30, 3, 14; "with details")]
fn long_failure_lists_are_capped_and_name_how_many_more_are_in_the_log(count: usize, details: usize, shown: usize) {
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

#[test]
fn each_failure_keeps_at_most_max_details_lines() {
    let logs: String = (0..MAX_DETAILS + 3)
        .map(|i| format!("    t_test.go:{i}: bad {i}\n"))
        .collect();
    let summary = summarize_test_failures(&format!("--- FAIL: TestMany (0.00s)\n{logs}")).unwrap();
    let details: Vec<&str> = summary.lines().filter(|l| l.starts_with("    ")).collect();
    assert_eq!(details.len(), MAX_DETAILS);
    assert_eq!(details[0], "    t_test.go:0: bad 0");
}

#[test]
fn tracking_stops_at_max_tracked_tests_but_still_counts_and_caps_the_listing() {
    let output: String = (0..MAX_TRACKED + 10)
        .map(|i| format!("--- FAIL: Test{i} (0.00s)\n"))
        .collect();
    let summary = summarize_test_failures(&output).unwrap();
    let lines: Vec<&str> = summary.lines().collect();
    assert_eq!(lines[0], format!("[{MAX_TRACKED} failing tests; details from log]"));
    assert_eq!(
        lines.last(),
        Some(&format!("[{} more failing tests in the log]", MAX_TRACKED - MAX_FAILURES).as_str())
    );
}

#[test]
fn a_detail_line_longer_than_the_line_limit_is_truncated() {
    let long = "x".repeat(crate::tools::truncate::GREP_MAX_LINE_LENGTH * 2);
    let summary = summarize_test_failures(&format!("--- FAIL: TestLong (0.00s)\n    t_test.go:1: {long}\n")).unwrap();
    let detail = summary.lines().last().unwrap().trim_start();
    assert!(detail.starts_with("t_test.go:1: xxx"), "{detail}");
    assert!(detail.ends_with("... [truncated]"), "{detail}");
    assert!(detail.len() < long.len());
}

#[test]
fn ansi_colored_output_is_parsed_like_plain_output() {
    let colored = "test math::subtracts ... \u{1b}[31mFAILED\u{1b}[0m\n\n\
        ---- math::subtracts stdout ----\n\
        thread 'math::subtracts' panicked at src/math.rs:12:9:\n\
        \u{1b}[1mboom\u{1b}[0m\n";
    assert_eq!(
        summarize_test_failures(colored).as_deref(),
        Some(
            "[1 failing test; details from log]\n- math::subtracts\n    thread 'math::subtracts' panicked at src/math.rs:12:9:\n    boom"
        )
    );
}

#[test]
fn nextest_fail_lines_name_the_test_with_or_without_a_counter() {
    let output = "        FAIL [   0.004s] demo tests::plain\n        FAIL [   0.004s] (1/3) demo tests::counted\n";
    assert_eq!(
        summarize_test_failures(output).as_deref(),
        Some("[2 failing tests; details from log]\n- demo tests::plain\n- demo tests::counted")
    );
}

#[test]
fn same_named_tests_of_different_crates_stay_separate_and_keep_their_own_details() {
    let output = "\
        FAIL [   0.004s] a tests::it
        FAIL [   0.004s] b tests::it
--- STDOUT:              b tests::it ---
running 1 test
test tests::it ... FAILED

---- tests::it stdout ----
thread 'tests::it' panicked at b/src/lib.rs:1:1:
boom b
";
    assert_eq!(
        summarize_test_failures(output).as_deref(),
        Some(
            "[2 failing tests; details from log]\n- a tests::it\n- b tests::it\n    thread 'tests::it' panicked at b/src/lib.rs:1:1:\n    boom b"
        )
    );
}

#[test_case("demo tests::it", "tests::it", true; "crate prefix")]
#[test_case("tests/t.py::test_a", "test_a", true; "python path")]
#[test_case("src/a.test.ts > math > sub", "sub", true; "js suite")]
#[test_case("a tests::it", "b tests::it", false; "different crates")]
#[test_case("mytests::it", "tests::it", false; "no module boundary")]
#[test_case("xtest_a", "test_a", false; "no name boundary")]
fn names_merge_only_across_a_path_or_suite_boundary(a: &str, b: &str, merged: bool) {
    assert_eq!(same_test(a, b), merged);
}
