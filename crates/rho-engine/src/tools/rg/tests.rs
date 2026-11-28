mod bounds;
mod search;
#[cfg(unix)]
mod unreadable;

use super::*;
use rho_test_support::TestDir;
use std::path::Path;

pub(crate) fn fixture() -> TestDir {
    use crate::test_support::write_file;
    let dir = crate::test_support::sample_search_tree();
    write_file(
        &dir.join("src/ui/widget.rs"),
        "pub struct Widget;\npub enum Kind { A, B }\n",
    );
    write_file(&dir.join("README.md"), "# Fixture\ntodo list\npub markdown\n");
    write_file(&dir.join(".hidden_file"), "hidden todo\n");
    dir
}

pub(crate) async fn search(dir: &impl AsRef<Path>, pattern: &str, mutate: impl FnOnce(&mut RgArgs)) -> ToolResult {
    let mut args = RgArgs {
        pattern: pattern.to_string(),
        path: None,
        file_type: None,
        hidden: None,
        output_mode: None,
        context: None,
        multiline: None,
        offset: None,
        limit: None,
    };
    mutate(&mut args);
    RgTool::new(dir).execute(args).await.unwrap()
}
