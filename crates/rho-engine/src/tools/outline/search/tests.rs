use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_outline_single_rust_file() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());
    let file_path = temp.path().join("main.rs");
    fs::write(
        &file_path,
        r#"
pub struct Service;

impl Service {
    pub fn new() -> Self {
        Self
    }
}
"#,
    )
    .unwrap();

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "main.rs",
            query: None,
            kind: None,
            depth: None,
        },
    )
    .unwrap();

    assert!(!res.is_error);
    let output = res.content;
    assert!(output.starts_with("main.rs:"));
    assert!(output.contains("  line 2: pub struct Service"));
    assert!(output.contains("  line 4: impl Service"));
    assert!(output.contains("    line 5: pub fn new() -> Self"));
}

#[test]
fn test_outline_unsupported_file_extension() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());
    let file_path = temp.path().join("readme.md");
    fs::write(&file_path, "# Hello").unwrap();

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "readme.md",
            query: None,
            kind: None,
            depth: None,
        },
    )
    .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("Syntax outline not supported for extension '.md'"));
}

#[test]
fn test_outline_directory_with_query_and_gitignore() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());

    // Create .git and .gitignore
    fs::create_dir_all(temp.path().join(".git")).unwrap();
    fs::write(temp.path().join(".gitignore"), "ignored.rs\n").unwrap();

    // Create source files
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        "pub fn target_function() {}\npub fn other_function() {}",
    )
    .unwrap();
    fs::write(src_dir.join("ignored.rs"), "pub fn target_function() {}").unwrap();
    fs::write(src_dir.join(".hidden.rs"), "pub fn target_function() {}").unwrap();

    // Search directory for "target"
    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "src",
            query: Some("target"),
            kind: None,
            depth: None,
        },
    )
    .unwrap();

    assert!(!res.is_error);
    let output = res.content;
    assert!(output.contains("src/lib.rs:"));
    assert!(output.contains("target_function"));
    assert!(!output.contains("other_function"));
    assert!(!output.contains("ignored.rs"));
    assert!(!output.contains(".hidden.rs"));
}

#[test]
fn test_outline_kind_filter() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());
    let file_path = temp.path().join("types.ts");
    fs::write(
        &file_path,
        r#"
export interface User {
    id: string;
}

export function getUser(): User {
    return { id: "1" };
}
"#,
    )
    .unwrap();

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "types.ts",
            query: None,
            kind: Some("interface"),
            depth: None,
        },
    )
    .unwrap();

    assert!(!res.is_error);
    let output = res.content;
    assert!(output.contains("export interface User"));
    assert!(!output.contains("getUser"));
}

#[test]
fn test_outline_depth_filter() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());
    let file_path = temp.path().join("app.py");
    fs::write(
        &file_path,
        r#"
class App:
    def run(self):
        pass
"#,
    )
    .unwrap();

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "app.py",
            query: None,
            kind: None,
            depth: Some(0),
        },
    )
    .unwrap();

    assert!(!res.is_error);
    let output = res.content;
    assert!(output.contains("class App:"));
    assert!(!output.contains("def run"));
}

#[test]
fn test_outline_no_matches_found() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());
    let file_path = temp.path().join("main.rs");
    fs::write(&file_path, "pub fn foo() {}").unwrap();

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "main.rs",
            query: Some("nonexistent"),
            kind: None,
            depth: None,
        },
    )
    .unwrap();

    assert!(!res.is_error);
    assert_eq!(res.content, "No matching symbols found");
}

#[test]
fn test_outline_path_outside_workspace_errors() {
    let temp = tempdir().unwrap();
    let ws = Workspace::new(temp.path());

    let res = search_outline(
        &ws,
        OutlineSearchOptions {
            path: "/etc/passwd",
            query: None,
            kind: None,
            depth: None,
        },
    );

    assert!(res.is_err());
    assert!(res.unwrap_err().contains("outside the workspace"));
}
