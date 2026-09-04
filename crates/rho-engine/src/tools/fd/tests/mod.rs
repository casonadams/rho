mod bounds;
mod search;
mod stats;

use super::*;
use tempfile::TempDir;

pub(crate) fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod ui;\n").unwrap();
    std::fs::write(dir.path().join("src/ui/widget.rs"), "pub struct Widget;\n").unwrap();
    std::fs::write(dir.path().join("README.md"), "# Fixture\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "secret notes\n").unwrap();
    std::fs::write(dir.path().join(".hidden_file"), "x\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "notes.txt\n").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

pub(crate) async fn find(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut FdArgs)) -> ToolResult {
    let mut args = FdArgs {
        pattern: Some(pattern.to_string()),
        ..Default::default()
    };
    mutate(&mut args);
    FdTool::new(dir.path()).execute(args).await.unwrap()
}
