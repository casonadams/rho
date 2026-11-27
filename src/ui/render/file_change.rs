//! Pre-run file snapshots and header stats for edit and write tool cards.

pub(crate) use rho_engine::tools::file_change::{capture, edit_start_lines, explicit_edit_line, read_bounded};
use rho_harness_core::presentation::{FileChange, ToolLine};
use serde_json::Value;

pub(crate) fn removed_text(edit: &Value, idx: usize, change: Option<&FileChange>) -> String {
    if let Some(old) = edit.get("oldText").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return old.to_string();
    }
    if let Some(region) = change
        .and_then(|c| c.replaced_regions.get(idx))
        .and_then(Option::as_ref)
    {
        return region.clone();
    }
    let anchor = |key: &str| edit.get(key).and_then(Value::as_str).unwrap_or("");
    match (anchor("startText"), anchor("endText")) {
        ("", "") => String::new(),
        (start, end) => format!("{start}\n\u{22ee}\n{end}"),
    }
}

pub(crate) fn edit_line_stats(args: &Value, change: Option<&FileChange>) -> Option<(usize, usize, usize)> {
    let edits = args.get("edits")?.as_array()?;
    let lines = |text: &str| text.lines().count();
    let removed = edits
        .iter()
        .enumerate()
        .map(|(idx, edit)| lines(&removed_text(edit, idx, change)))
        .sum();
    let added = edits
        .iter()
        .map(|edit| edit.get("newText").and_then(Value::as_str).map_or(0, lines))
        .sum();
    Some((edits.len(), added, removed))
}

pub(crate) fn format_size(bytes: usize) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

pub(crate) fn header_details(tool: &ToolLine) -> Option<String> {
    if tool.is_error {
        return None;
    }
    match tool.name.as_str() {
        "edit" => {
            let (edits, added, removed) = edit_line_stats(&tool.arguments, tool.file_change.as_ref())?;
            Some(format!("{} \u{b7} +{added} -{removed}", plural(edits, "edit")))
        }
        "write" => {
            let content = rho_harness_core::args::write_text(&tool.arguments)?;
            let state = match tool.file_change.as_ref().map(|change| change.overwrote_existing) {
                Some(true) => " \u{b7} overwrote",
                Some(false) => " \u{b7} new file",
                None => "",
            };
            Some(format!(
                "{} \u{b7} {}{state}",
                plural(content.lines().count(), "line"),
                format_size(content.len())
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, arguments: Value, file_change: Option<FileChange>) -> ToolLine {
        let mut line = ToolLine::new(name, arguments);
        line.file_change = file_change;
        line
    }

    #[test]
    fn edit_header_counts_added_and_removed_lines() {
        let args = json!({"path": "a.rs", "edits": [
            {"oldText": "a\nb", "newText": "c"},
            {"oldText": "", "newText": "x\ny\nz"},
        ]});
        let details = header_details(&tool("edit", args, None)).unwrap();
        assert_eq!(details, "2 edits \u{b7} +4 -2");
        let single = json!({"edits": [{"oldText": "a", "newText": "b"}]});
        assert_eq!(
            header_details(&tool("edit", single, None)).unwrap(),
            "1 edit \u{b7} +1 -1"
        );
    }

    #[test]
    fn write_header_reports_lines_size_and_overwrite_state() {
        let args = json!({"path": "a.txt", "content": "one\ntwo\nthree\n"});
        let cases = [
            (None, "3 lines \u{b7} 14 B"),
            (
                Some(FileChange {
                    overwrote_existing: false,
                    edit_start_lines: vec![],
                    replaced_regions: vec![],
                }),
                "3 lines \u{b7} 14 B \u{b7} new file",
            ),
            (
                Some(FileChange {
                    overwrote_existing: true,
                    edit_start_lines: vec![],
                    replaced_regions: vec![],
                }),
                "3 lines \u{b7} 14 B \u{b7} overwrote",
            ),
        ];
        for (change, expected) in cases {
            assert_eq!(header_details(&tool("write", args.clone(), change)).unwrap(), expected);
        }
    }

    #[test]
    fn errors_and_other_tools_get_no_details() {
        let mut failed = tool("write", json!({"content": "x"}), None);
        failed.is_error = true;
        assert!(header_details(&failed).is_none());
        assert!(header_details(&tool("rg", json!({}), None)).is_none());
    }

    #[test]
    fn format_size_uses_binary_units() {
        let sizes = [0, 1023, 1024, 1536, 1_048_576, 5 * 1_048_576];
        let shown: Vec<String> = sizes.into_iter().map(format_size).collect();
        assert_eq!(shown, ["0 B", "1023 B", "1.0 KB", "1.5 KB", "1.0 MB", "5.0 MB"]);
    }

    #[test]
    fn capture_records_overwrite_state_and_edit_lines_before_the_tool_runs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();
        let path = file.to_str().unwrap();

        let (key, write) = capture("write", &json!({"path": path, "content": "x"})).unwrap();
        assert_eq!((key.as_str(), write.overwrote_existing), (path, true));

        let missing = dir.path().join("none.txt");
        let (_, fresh) = capture("write", &json!({"path": missing.to_str().unwrap()})).unwrap();
        assert!(!fresh.overwrote_existing);

        let edits = json!([
            {"oldText": "gamma", "newText": "delta"},
            {"oldText": "beta", "newText": "b", "line": 40},
            {"oldText": "absent", "newText": "z"},
        ]);
        let (_, edit) = capture("edit", &json!({"path": path, "edits": edits})).unwrap();
        assert_eq!(edit.edit_start_lines, [Some(3), Some(40), None]);
        assert_eq!(edit.replaced_regions, [None, None, None]);
        assert!(capture("rg", &json!({"path": path})).is_none());
    }

    #[test]
    fn region_edits_capture_the_removed_block_for_the_card() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.rs");
        std::fs::write(&file, "use a;\nfn old() {\n    a();\n}\nfn keep() {}\n").unwrap();
        let args = json!({"path": file.to_str().unwrap(), "edits": [
            {"oldText": "use a;", "newText": "use b;"},
            {"startText": "fn old() {", "endText": "}\n", "newText": ""},
        ]});

        let (_, change) = capture("edit", &args).unwrap();

        assert_eq!(change.edit_start_lines, [Some(1), Some(2)]);
        let region = "fn old() {\n    a();\n}\n".to_string();
        assert_eq!(change.replaced_regions, [None, Some(region)]);
        let card = tool("edit", args.clone(), Some(change));
        assert_eq!(header_details(&card).unwrap(), "2 edits \u{b7} +1 -4");
        assert_eq!(removed_text(&args["edits"][1], 1, None), "fn old() {\n\u{22ee}\n}\n");
    }
}
