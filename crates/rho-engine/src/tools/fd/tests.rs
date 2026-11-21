mod bounds;
mod search;
mod stats;

use super::*;
use tempfile::TempDir;

pub(crate) fn fixture() -> TempDir {
    crate::test_support::sample_search_tree()
}

pub(crate) async fn find(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut FdArgs)) -> ToolResult {
    let mut args = FdArgs {
        pattern: Some(pattern.to_string()),
        ..Default::default()
    };
    mutate(&mut args);
    FdTool::new(dir.path()).execute(args).await.unwrap()
}
