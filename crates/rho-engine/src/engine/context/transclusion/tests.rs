use super::*;
use std::fs;

#[test]
fn test_basic_transclusion() {
    let temp = tempfile::tempdir().unwrap();
    let sub = temp.path().join("sub.md");
    fs::write(&sub, "Inlined content from sub.md").unwrap();

    let content = "# Header\n@sub.md\nFooter";
    let expanded = expand_transclusions(content, temp.path());
    assert_eq!(expanded, "# Header\nInlined content from sub.md\nFooter");
}

#[test]
fn test_transclusion_with_whitespace_and_comment() {
    let temp = tempfile::tempdir().unwrap();
    let sub = temp.path().join("standards.md");
    fs::write(&sub, "Strict typing rules").unwrap();

    let content = "Rules:\n  @standards.md # Project standards\nDone";
    let expanded = expand_transclusions(content, temp.path());
    assert_eq!(expanded, "Rules:\nStrict typing rules\nDone");
}

#[test]
fn test_code_fences_are_not_transcluded() {
    let temp = tempfile::tempdir().unwrap();
    let content = "```python\n@app.route('/')\ndef index(): pass\n```";
    let expanded = expand_transclusions(content, temp.path());
    assert_eq!(expanded, content);
}

#[test]
fn test_missing_file_transclusion_fails_gracefully() {
    let temp = tempfile::tempdir().unwrap();
    let content = "Before\n@nonexistent.md\nAfter";
    let expanded = expand_transclusions(content, temp.path());
    assert!(expanded.contains("<!-- Transclusion failed: file not found: nonexistent.md -->"));
    assert!(expanded.starts_with("Before\n"));
    assert!(expanded.ends_with("\nAfter"));
}

#[test]
fn test_recursive_transclusion_up_to_depth_limit() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    fs::write(dir.join("c.md"), "Level 3 content").unwrap();
    fs::write(dir.join("b.md"), "Level 2:\n@c.md").unwrap();
    fs::write(dir.join("a.md"), "Level 1:\n@b.md").unwrap();

    let content = "@a.md";
    let expanded = expand_transclusions(content, dir);
    assert_eq!(expanded, "Level 1:\nLevel 2:\nLevel 3 content");
}

#[test]
fn test_transclusion_depth_limit_exceeded() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    fs::write(dir.join("d.md"), "Level 4").unwrap();
    fs::write(dir.join("c.md"), "@d.md").unwrap();
    fs::write(dir.join("b.md"), "@c.md").unwrap();
    fs::write(dir.join("a.md"), "@b.md").unwrap();

    let content = "@a.md";
    let expanded = expand_transclusions(content, dir);
    assert!(expanded.contains("<!-- Transclusion depth limit exceeded: d.md -->"));
}

#[test]
fn test_circular_transclusion_cycle_detected() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    fs::write(dir.join("cycle_a.md"), "From A:\n@cycle_b.md").unwrap();
    fs::write(dir.join("cycle_b.md"), "From B:\n@cycle_a.md").unwrap();

    let content = "@cycle_a.md";
    let expanded = expand_transclusions(content, dir);
    assert!(expanded.contains("<!-- Transclusion loop detected: cycle_a.md -->"));
}

#[test]
fn test_self_transclusion_cycle_detected_with_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("AGENTS.md");
    fs::write(&root, "Self:\n@AGENTS.md").unwrap();

    let content = "Self:\n@AGENTS.md";
    let expanded = expand_transclusions_with_root(content, temp.path(), Some(&root));
    assert!(expanded.contains("<!-- Transclusion loop detected: AGENTS.md -->"));
}

#[test]
fn test_large_file_truncation() {
    let temp = tempfile::tempdir().unwrap();
    let large = temp.path().join("large.txt");
    let content_65k = "a".repeat(65 * 1024);
    fs::write(&large, &content_65k).unwrap();

    let content = "@large.txt";
    let expanded = expand_transclusions(content, temp.path());
    assert!(expanded.contains("<!-- Transclusion truncated at 64 KB: large.txt -->"));
    assert_eq!(expanded.lines().next().unwrap().len(), MAX_TRANSCLUSION_BYTES);
}

#[test]
fn test_transclusion_outside_workspace_and_home_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let secret = external.path().join("secret.txt");
    fs::write(&secret, "sensitive data").unwrap();

    let secret_path = secret.display().to_string();
    let content = format!("@{}", secret_path);
    let expanded = expand_transclusions(&content, workspace.path());
    assert!(expanded.contains(&format!(
        "<!-- Transclusion failed: path not permitted: {secret_path} -->"
    )));
}

#[test]
fn test_transclusion_git_internal_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let git_dir = workspace.path().join(".git");
    fs::create_dir_all(&git_dir).unwrap();
    let config = git_dir.join("config");
    fs::write(&config, "[core]\nrepositoryformatversion = 0").unwrap();

    let content = "@.git/config";
    let expanded = expand_transclusions(content, workspace.path());
    assert!(expanded.contains("<!-- Transclusion failed: path not permitted: .git/config -->"));
}
