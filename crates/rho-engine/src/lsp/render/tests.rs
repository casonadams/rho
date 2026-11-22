use super::*;
use serde_json::json;

fn uri(path: &Path) -> String {
    super::super::client::file_uri(path)
}

#[test]
fn locations_accept_location_lists_and_links_sorted_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("src/b.rs");
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    std::fs::write(&a, "fn one() {}\n  fn two() {}\n").unwrap();
    std::fs::write(&b, "use crate::two;\n").unwrap();
    let range =
        |line: u32| json!({ "start": { "line": line, "character": 2 }, "end": { "line": line, "character": 5 } });
    let response = json!([
        { "uri": uri(&b), "range": range(0) },
        { "targetUri": uri(&a), "targetRange": range(0), "targetSelectionRange": range(1) },
        { "uri": uri(&a), "range": range(1) },
    ]);

    let found = locations(&response);
    assert_eq!(found.len(), 2);
    assert_eq!(
        format_locations(dir.path(), "reference", &found),
        "2 references\n\na.rs\n2:fn two() {}\n\nsrc/b.rs\n1:use crate::two;"
    );
    assert_eq!(locations(&json!({ "uri": uri(&a), "range": range(0) })).len(), 1);
    assert_eq!(format_locations(dir.path(), "definition", &[]), "No definition found");
}

fn diag(severity: u8, line: u32, message: &str) -> Diagnostic {
    Diagnostic {
        line,
        column: 3,
        severity,
        message: message.to_string(),
        source: Some("rustc".into()),
        code: Some("E0308".into()),
    }
}

#[test]
fn diagnostics_sort_errors_first_dedupe_and_keep_the_first_message_line() {
    let mut list = vec![
        diag(2, 1, "unused"),
        diag(1, 9, "mismatched types\nexpected u8"),
        diag(1, 4, "missing"),
        diag(1, 4, "missing"),
    ];
    sort_diagnostics(&mut list);
    assert_eq!(count_summary(&list), "2 errors, 1 warning");
    assert_eq!(
        diagnostic_rows(&list),
        "4:3 error [rustc] missing (E0308)\n9:3 error [rustc] mismatched types (E0308)\n1:3 warning [rustc] unused (E0308)"
    );
    let many: Vec<_> = (0..25).map(|line| diag(1, line, "x")).collect();
    assert!(diagnostic_rows(&many).ends_with("[5 more not shown]"));
}

#[test]
fn outlines_nest_children_and_symbol_paths_match_their_suffix() {
    let range =
        |a: u32, b: u32| json!({ "start": { "line": a, "character": 0 }, "end": { "line": b, "character": 1 } });
    let response = json!([
        { "name": "Config", "kind": 23, "range": range(0, 9), "children": [
            { "name": "load", "kind": 6, "detail": "fn(&Path) -> Config", "range": range(2, 5) },
        ] },
        { "name": "load", "kind": 12, "range": range(11, 14) },
    ]);
    let entries = document_symbols(&response);
    assert_eq!(
        format_outline(&entries),
        "1-10 struct Config\n3-6   method load  fn(&Path) -> Config\n12-15 function load"
    );
    for (query, expected) in [("load", 2), ("Config.load", 1), ("Config::load", 1), ("Other.load", 0)] {
        assert_eq!(matching_symbols(&entries, query).len(), expected, "{query}");
    }
    assert_eq!(matching_symbols(&entries, "Config.load")[0].start, 3);

    let flat = json!([{ "name": "run", "kind": 12, "containerName": "Engine",
        "location": { "uri": "file:///x.rs", "range": range(4, 8) } }]);
    assert_eq!(matching_symbols(&document_symbols(&flat), "Engine.run").len(), 1);

    let impls = json!([
        { "name": "impl Config", "kind": 19, "range": range(0, 9), "children": [
            { "name": "load", "kind": 12, "range": range(1, 3) }] },
        { "name": "impl Display for Config<T>", "kind": 19, "range": range(10, 19), "children": [
            { "name": "fmt", "kind": 12, "range": range(11, 13) }] },
    ]);
    let entries = document_symbols(&impls);
    for (query, expected) in [
        ("Config.load", 1),
        ("Config::fmt", 1),
        ("Display.fmt", 1),
        ("Display.load", 0),
    ] {
        assert_eq!(matching_symbols(&entries, query).len(), expected, "{query}");
    }
}

#[test]
fn hover_reads_every_content_shape() {
    assert_eq!(
        hover_text(&json!({ "contents": { "kind": "markdown", "value": " `u8` " } })),
        "`u8`"
    );
    assert_eq!(
        hover_text(&json!({ "contents": ["a", { "language": "rust", "value": "b" }] })),
        "a\n\nb"
    );
    assert_eq!(hover_text(&json!(null)), "No hover information");
}

#[test]
fn text_edits_apply_from_the_end_in_the_negotiated_encoding() {
    let text = "let \u{1F600} = old;\nold();\n";
    let edit = |line: u32, start: u32, end: u32| json!({ "range": { "start": { "line": line, "character": start }, "end": { "line": line, "character": end } }, "newText": "new" });
    let utf16 = apply_text_edits(text, &[edit(0, 9, 12), edit(1, 0, 3)], PositionEncoding::Utf16).unwrap();
    assert_eq!(utf16, "let \u{1F600} = new;\nnew();\n");
    let utf8 = apply_text_edits(text, &[edit(0, 11, 14)], PositionEncoding::Utf8).unwrap();
    assert_eq!(utf8, "let \u{1F600} = new;\nold();\n");
    let overlap = apply_text_edits(text, &[edit(1, 0, 3), edit(1, 1, 2)], PositionEncoding::Utf8);
    assert!(overlap.unwrap_err().contains("overlapping"));

    let inserts = apply_text_edits(
        "ab\r\n",
        &[edit(0, 1, 1), edit(0, 1, 1), edit(0, 9, 9)],
        PositionEncoding::Utf8,
    );
    assert_eq!(
        inserts.unwrap(),
        "anewnewbnew\r\n",
        "same-position inserts keep their order; past-the-end stays before CRLF"
    );
}

#[test]
fn workspace_edits_prefer_document_changes_and_reject_file_operations() {
    let edits = workspace_edits(&json!({
        "changes": { "file:///a.rs": [{ "newText": "x" }] },
        "documentChanges": [{ "textDocument": { "uri": "file:///b.rs", "version": 1 }, "edits": [{ "newText": "y" }] }],
    }))
    .unwrap();
    assert_eq!(edits.keys().collect::<Vec<_>>(), [&PathBuf::from("/b.rs")]);
    let only_changes = workspace_edits(&json!({ "changes": { "file:///a.rs": [{ "newText": "x" }] } })).unwrap();
    assert_eq!(only_changes[&PathBuf::from("/a.rs")].len(), 1);
    let rename = workspace_edits(
        &json!({ "documentChanges": [{ "kind": "rename", "oldUri": "file:///a", "newUri": "file:///b" }] }),
    );
    assert!(rename.unwrap_err().contains("rename operation"));
}
