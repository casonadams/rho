mod bounds;
mod search;
mod stats;

use super::*;
use rho_test_support::TestDir;

pub(crate) fn fixture() -> TestDir {
    crate::test_support::sample_search_tree()
}

pub(crate) async fn find(dir: &TestDir, pattern: &str, mutate: impl FnOnce(&mut FdArgs)) -> ToolResult {
    let mut args = FdArgs {
        pattern: Some(pattern.to_string()),
        ..Default::default()
    };
    mutate(&mut args);
    FdTool::new(dir).execute(args).await.unwrap()
}
