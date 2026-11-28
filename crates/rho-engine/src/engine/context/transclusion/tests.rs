use super::*;
use crate::test_support::TestDir;
use std::fs;
use test_case::test_case;

fn expand_transclusions(content: &str, base_dir: &Path) -> String {
    expand_transclusions_with_root(content, base_dir, None)
}

fn write_files(dir: &Path, files: &[(&str, &str)]) {
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
}

#[test_case(
    "sub.md",
    "Inlined content from sub.md",
    "# Header\n@sub.md\nFooter",
    "# Header\nInlined content from sub.md\nFooter";
    "plain directive"
)]
#[test_case(
    "standards.md",
    "Strict typing rules",
    "Rules:\n  @standards.md # Project standards\nDone",
    "Rules:\nStrict typing rules\nDone";
    "whitespace and comment"
)]
fn referenced_file_replaces_its_directive(file: &str, body: &str, content: &str, expected: &str) {
    let temp = TestDir::new();
    write_files(&temp, &[(file, body)]);
    let expanded = expand_transclusions(content, &temp);
    assert_eq!(expanded, expected);
}

#[test]
fn test_code_fences_are_not_transcluded() {
    let temp = TestDir::new();
    let content = "```python\n@app.route('/')\ndef index(): pass\n```";
    let expanded = expand_transclusions(content, &temp);
    assert_eq!(expanded, content);
}

#[test]
fn test_missing_file_transclusion_fails_gracefully() {
    let temp = TestDir::new();
    let content = "Before\n@nonexistent.md\nAfter";
    let expanded = expand_transclusions(content, &temp);
    assert!(expanded.contains("<!-- Transclusion failed: file not found: nonexistent.md -->"));
    assert!(expanded.starts_with("Before\n"));
    assert!(expanded.ends_with("\nAfter"));
}

#[test]
fn test_recursive_transclusion_up_to_depth_limit() {
    let temp = TestDir::new();
    write_files(
        &temp,
        &[
            ("c.md", "Level 3 content"),
            ("b.md", "Level 2:\n@c.md"),
            ("a.md", "Level 1:\n@b.md"),
        ],
    );

    let expanded = expand_transclusions("@a.md", &temp);
    assert_eq!(expanded, "Level 1:\nLevel 2:\nLevel 3 content");
}

#[test]
fn test_transclusion_depth_limit_exceeded() {
    let temp = TestDir::new();
    write_files(
        &temp,
        &[
            ("d.md", "Level 4"),
            ("c.md", "@d.md"),
            ("b.md", "@c.md"),
            ("a.md", "@b.md"),
        ],
    );

    let expanded = expand_transclusions("@a.md", &temp);
    assert!(expanded.contains("<!-- Transclusion depth limit exceeded: d.md -->"));
}

#[test]
fn test_circular_transclusion_cycle_detected() {
    let temp = TestDir::new();
    write_files(
        &temp,
        &[
            ("cycle_a.md", "From A:\n@cycle_b.md"),
            ("cycle_b.md", "From B:\n@cycle_a.md"),
        ],
    );

    let expanded = expand_transclusions("@cycle_a.md", &temp);
    assert!(expanded.contains("<!-- Transclusion loop detected: cycle_a.md -->"));
}

#[test]
fn test_self_transclusion_cycle_detected_with_root() {
    let temp = TestDir::new();
    write_files(&temp, &[("AGENTS.md", "Self:\n@AGENTS.md")]);
    let root = temp.join("AGENTS.md");

    let content = "Self:\n@AGENTS.md";
    let expanded = expand_transclusions_with_root(content, &temp, Some(&root));
    assert!(expanded.contains("<!-- Transclusion loop detected: AGENTS.md -->"));
}

#[test]
fn test_large_file_truncation() {
    let temp = TestDir::new();
    let content_65k = "a".repeat(65 * 1024);
    write_files(&temp, &[("large.txt", content_65k.as_str())]);

    let expanded = expand_transclusions("@large.txt", &temp);
    assert!(expanded.contains("<!-- Transclusion truncated at 64 KB: large.txt -->"));
    assert_eq!(expanded.lines().next().unwrap().len(), MAX_TRANSCLUSION_BYTES);
}

#[test]
fn test_transclusion_outside_workspace_and_home_rejected() {
    let workspace = TestDir::new();
    let external = TestDir::new();
    write_files(&external, &[("secret.txt", "sensitive data")]);
    let secret = external.join("secret.txt");

    let secret_path = secret.display().to_string();
    let content = format!("@{}", secret_path);
    let expanded = expand_transclusions(&content, &workspace);
    assert!(expanded.contains(&format!(
        "<!-- Transclusion failed: path not permitted: {secret_path} -->"
    )));
}

#[test]
fn test_transclusion_git_internal_rejected() {
    let workspace = TestDir::new();
    fs::create_dir_all(workspace.join(".git")).unwrap();
    write_files(&workspace, &[(".git/config", "[core]\nrepositoryformatversion = 0")]);

    let expanded = expand_transclusions("@.git/config", &workspace);
    assert!(expanded.contains("<!-- Transclusion failed: path not permitted: .git/config -->"));
}

#[test]
fn test_transclusion_credential_store_rejected() {
    let (config_dir, _env) = crate::permission::tests::config_home();
    let workspace = TestDir::new();
    let auth = config_dir.join("auth.json");
    assert!(!is_confined_target(&auth, &workspace));
}
