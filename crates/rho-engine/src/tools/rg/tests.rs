mod bounds;
mod search;

use super::*;
use tempfile::TempDir;

pub(crate) fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod ui;\n").unwrap();
    std::fs::write(
        dir.path().join("src/ui/widget.rs"),
        "pub struct Widget;\npub enum Kind { A, B }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("README.md"), "# Fixture\ntodo list\npub markdown\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "secret notes\n").unwrap();
    std::fs::write(dir.path().join(".hidden_file"), "hidden todo\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "notes.txt\n").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

pub(crate) async fn search(dir: &TempDir, pattern: &str, mutate: impl FnOnce(&mut RgArgs)) -> ToolResult {
    let mut args = RgArgs {
        pattern: pattern.to_string(),
        path: None,
        file_type: None,
        hidden: None,
        limit: None,
    };
    mutate(&mut args);
    RgTool::new(dir.path()).execute(args).await.unwrap()
}
