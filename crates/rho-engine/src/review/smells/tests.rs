use super::*;

fn found(path: &str, text: &str) -> Vec<(usize, &'static str)> {
    let mut found: Vec<_> = check(&Source::all_added(path, text))
        .iter()
        .map(|f| (f.line, f.rule))
        .collect();
    found.sort_unstable();
    found
}

fn repeat_lines(n: usize, line: &str) -> String {
    (0..n).map(|_| format!("{line}\n")).collect()
}

type Case = (&'static str, String, Vec<(usize, &'static str)>);

#[test]
fn line_rules_fire_on_positive_and_stay_quiet_on_negative_cases() {
    let long = format!("let text = compute(1, 2); // {}", "word ".repeat(30));
    let cases: Vec<Case> = vec![
        (
            "a.rs",
            "fn a() {\n    dbg!(x);\n}\n".into(),
            vec![(2, "debug-leftover")],
        ),
        ("a.rs", "let s = \"dbg!(x)\";\n".into(), vec![]),
        ("a.rs", "// dbg!(x) was here\n".into(), vec![]),
        (
            "a.rs",
            "#[allow(dead_code)]\n#![expect(unused)]\n".into(),
            vec![(1, "lint-suppression"), (2, "lint-suppression")],
        ),
        ("a.rs", "// #[allow(dead_code)]\n".into(), vec![]),
        (
            "a.rs",
            "fn a() {\n    todo!();\n    unimplemented!()\n}\n".into(),
            vec![(2, "placeholder"), (3, "placeholder")],
        ),
        ("tests/a.rs", "fn a() {\n    todo!();\n}\n".into(), vec![]),
        (
            "a.rs",
            "let v = x.unwrap();\nlet w = y.expect(\"no\");\nlet z = x.unwrap_or(1);\n".into(),
            vec![(1, "unwrap"), (2, "unwrap")],
        ),
        (
            "a.rs",
            "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        x.unwrap();\n    }\n}\n".into(),
            vec![],
        ),
        ("tests/a.rs", "let v = x.unwrap();\n".into(), vec![]),
        (
            "a.rs",
            "// TODO: later\n// XXX\nlet todo_list = 1;\n".into(),
            vec![(1, "todo-marker"), (2, "todo-marker")],
        ),
        (
            "a.rs",
            "// let x = foo(1);\n// }\n// Returns the value; see foo(1)\n/// let x = foo(1);\n// plain words;\n".into(),
            vec![(1, "commented-out-code")],
        ),
        ("a.rs", long.clone() + "\n", vec![(1, "long-line")]),
        (
            "a.rs",
            format!("// see https://example.com/{}\n", "a".repeat(150)),
            vec![],
        ),
        (
            "a.rs",
            "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n".into(),
            vec![(1, "conflict-marker"), (3, "conflict-marker"), (5, "conflict-marker")],
        ),
        ("a.md", "Title\n=======\n".into(), vec![]),
        ("a.js", "console.log(x);\n".into(), vec![(1, "console-log")]),
        ("a.test.js", "console.log(x);\n".into(), vec![]),
        (
            "a.ts",
            "// eslint-disable-next-line\n// @ts-ignore\n// @ts-expect-error\n// biome-ignore lint: x\ndebugger;\n"
                .into(),
            vec![
                (1, "lint-suppression"),
                (2, "lint-suppression"),
                (3, "lint-suppression"),
                (4, "lint-suppression"),
                (5, "debug-leftover"),
            ],
        ),
        (
            "a.py",
            "x = 1  # noqa\ny = 2  # type: ignore\n# pylint: disable=x\nbreakpoint()\npdb.set_trace()\n".into(),
            vec![
                (1, "lint-suppression"),
                (2, "lint-suppression"),
                (3, "lint-suppression"),
                (4, "debug-leftover"),
                (5, "debug-leftover"),
            ],
        ),
        (
            "a.py",
            "raise NotImplementedError\npass  # TODO\n".into(),
            vec![(1, "placeholder"), (2, "placeholder"), (2, "todo-marker")],
        ),
        (
            "a.go",
            "x := f() //nolint:errcheck\n".into(),
            vec![(1, "lint-suppression")],
        ),
        ("a.py", "print(\"# noqa\")\n".into(), vec![]),
    ];
    for (path, text, expected) in cases {
        assert_eq!(found(path, &text), expected, "{path}: {text}");
    }
}

#[test]
fn credentials_are_errors_except_in_test_code() {
    let key = format!("sk-{}", "a".repeat(24));
    let line = format!("let token = \"{key}\";\n");
    assert_eq!(found("a.rs", &line), [(1, "secret")]);
    assert_eq!(found("config.yaml", "password: \"hunter2hunter2\"\n"), [(1, "secret")]);
    assert_eq!(found("config.yaml", "password: \"${PASSWORD_ENV}\"\n"), []);
    assert_eq!(found("tests/a.rs", &line), []);
    assert_eq!(found("a.rs", "let sk = \"sk-short\";\n"), []);
}

#[test]
fn functions_are_checked_for_length_parameters_flags_and_nesting() {
    let body = repeat_lines(60, "    work();");
    let long = format!("fn a() {{\n{body}}}\n");
    let ok = format!("fn a() {{\n{}}}\n", repeat_lines(58, "    work();"));
    assert_eq!(found("a.rs", &long), [(1, "long-function")]);
    assert_eq!(found("a.rs", &ok), []);
    assert_eq!(found("tests/a.rs", &long), []);

    assert_eq!(
        found("a.rs", "fn a(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8) {}\n"),
        [(1, "too-many-params")]
    );
    assert_eq!(
        found("a.rs", "fn a(&self, a: u8, b: u8, c: u8, d: u8, e: u8, f: u8) {}\n"),
        []
    );
    assert_eq!(found("a.rs", "fn a(x: bool, y: bool) {}\n"), [(1, "bool-flags")]);
    assert_eq!(found("a.rs", "fn a(x: bool, y: u8) {}\n"), []);
    assert_eq!(
        found("a.py", "def a(x: bool = False, y: bool = True):\n    pass\n"),
        [(1, "bool-flags")]
    );
    assert_eq!(found("a.go", "func a(x bool, y bool) {\n}\n"), [(1, "bool-flags")]);
}

#[test]
fn only_nesting_deeper_than_five_levels_is_reported_once_per_function() {
    let nest = |depth: usize| {
        let mut text = String::from("fn a() {\n");
        for level in 1..=depth {
            text.push_str(&format!("{}if x{level} {{\n", "    ".repeat(level)));
        }
        text.push_str(&format!("{}work();\n", "    ".repeat(depth + 1)));
        text.push_str(&format!("{}}}\n}}\n", "    ".repeat(depth)));
        text
    };
    assert_eq!(found("a.rs", &nest(5)), []);
    assert_eq!(found("a.rs", &nest(6)), [(8, "deep-nesting")]);
    let py = "def a():\n    if a:\n        if b:\n            if c:\n                if d:\n                    if e:\n                        if f:\n                            pass\n";
    assert_eq!(found("a.py", py), [(8, "deep-nesting")]);
}

#[test]
fn only_added_lines_are_flagged() {
    let mut source = Source::all_added("a.rs", "dbg!(a);\ndbg!(b);\n");
    source.lines[0].added = false;
    let lines: Vec<usize> = check(&source).iter().map(|f| f.line).collect();
    assert_eq!(lines, [2]);
}

#[test]
fn new_files_over_400_lines_and_repeated_literals_are_flagged() {
    let big = repeat_lines(401, "let a = 1;");
    assert_eq!(found("a.rs", &big), [(1, "large-file")]);
    assert_eq!(found("a.rs", &repeat_lines(400, "let a = 1;")), []);
    assert_eq!(found("a.md", &big), []);

    let literal = "let a = \"some long literal\";\n";
    assert_eq!(found("a.rs", &repeat_lines(3, literal)), [(1, "repeated-literal")]);
    assert_eq!(found("a.rs", &repeat_lines(2, literal)), []);
    assert_eq!(found("a.rs", &repeat_lines(3, "let a = \"short\";")), []);
    assert_eq!(
        found("a.js", &repeat_lines(3, "f('another long literal');")),
        [(1, "repeated-literal")]
    );
    assert_eq!(found("tests/a.rs", &repeat_lines(3, literal)), []);
    assert_eq!(
        found(
            "a.rs",
            &repeat_lines(3, "a.get(\"first\").map(f).unwrap_or(\"second\");")
        ),
        []
    );
    assert_eq!(
        found(
            "a.rs",
            &repeat_lines(3, "#[serde(skip_serializing_if = \"Option::is_none\")]")
        ),
        []
    );
}
