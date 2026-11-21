mod bounds;
mod search;

use super::*;
use tempfile::TempDir;

pub(crate) fn fixture() -> TempDir {
    use crate::test_support::write_file;
    let dir = crate::test_support::sample_search_tree();
    write_file(
        &dir.path().join("src/ui/widget.rs"),
        "pub struct Widget;\npub enum Kind { A, B }\n",
    );
    write_file(&dir.path().join("README.md"), "# Fixture\ntodo list\npub markdown\n");
    write_file(&dir.path().join(".hidden_file"), "hidden todo\n");
    dir
}

pub(crate) async fn search(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut RgArgs)) -> ToolResult {
    let mut args = RgArgs {
        pattern: pattern.to_string(),
        path: None,
        file_type: None,
        hidden: None,
        output_mode: None,
        offset: None,
        limit: None,
    };
    mutate(&mut args);
    RgTool::new(dir.path()).execute(args).await.unwrap()
}
