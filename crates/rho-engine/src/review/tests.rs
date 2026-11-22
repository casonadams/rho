use super::*;

struct NoCorpus;

impl Corpus for NoCorpus {
    fn files(&self, _family: Lang) -> Vec<(String, String)> {
        Vec::new()
    }
}

fn new_file_diff(path: &str, lines: &[&str]) -> String {
    let body: String = lines.iter().map(|l| format!("+{l}\n")).collect();
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{body}",
        lines.len()
    )
}

#[test]
fn findings_render_errors_first_then_by_location() {
    let a = new_file_diff("a.rs", &["#[allow(dead_code)]", "fn a() {", "    dbg!(1);", "}"]);
    let b = new_file_diff("b.rs", &["// TODO: later", "let x = y.unwrap();"]);

    let review = review(&format!("{b}{a}"), &NoCorpus);

    assert_eq!(review.files_reviewed, 2);
    assert_eq!(review.counts(), (1, 3));
    assert_eq!(review.summary_line(), "review: 1 error, 3 warnings");
    let text = review.render(30);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Review: 1 error, 3 warnings in 2 files");
    assert_eq!(
        &lines[1..5],
        [
            "a.rs:3 [debug-leftover] `dbg!` left in the code",
            "a.rs:1 [lint-suppression] lint suppression: fix the cause instead of silencing the lint",
            "b.rs:1 [todo-marker] unfinished-work marker (TODO/FIXME/XXX/HACK)",
            "b.rs:2 [unwrap] `unwrap`/`expect` can panic in non-test code; propagate the error",
        ]
    );
    assert!(lines[5].starts_with("Fix errors first. Send the findings to the subagent by continuing it"));
    assert!(lines[5].contains("NOT run"));
}

#[test]
fn rendering_is_capped_and_says_how_many_findings_are_hidden() {
    let lines = vec!["dbg!(1);"; 5];
    let review = review(&new_file_diff("a.rs", &lines), &NoCorpus);

    let text = review.render(2);

    assert_eq!(text.matches("[debug-leftover]").count(), 2);
    assert!(text.contains("[3 more findings not shown]"), "{text}");
}

#[test]
fn a_clean_change_has_no_findings() {
    let review = review(
        &new_file_diff("a.rs", &["pub fn fine() -> u8 {", "    1", "}"]),
        &NoCorpus,
    );

    assert!(review.findings.is_empty());
    assert_eq!(review.summary_line(), "review: no findings");
    assert!(review.render(30).starts_with("Review: no findings\n"));
}

#[test]
fn lock_files_are_not_reviewed_and_documents_only_get_universal_rules() {
    let lock = new_file_diff("Cargo.lock", &["<<<<<<< ours"]);
    let doc = new_file_diff("README.md", &["TODO: write", "dbg!(x)", "<<<<<<< ours"]);

    let review = review(&format!("{lock}{doc}"), &NoCorpus);

    assert_eq!(review.files_reviewed, 1);
    let rules: Vec<&str> = review.findings.iter().map(|f| f.rule).collect();
    assert_eq!(rules, ["conflict-marker"]);
}
