use super::*;
use crate::tools::edit::{EditArgs, EditTool};
use crate::tools::write::{WriteArgs, WriteTool};

struct Tools {
    dir: tempfile::TempDir,
    edit: EditTool,
    write: WriteTool,
    undo: UndoTool,
}

fn tools() -> Tools {
    let dir = tempfile::tempdir().unwrap();
    let history = EditHistory::default();
    Tools {
        edit: EditTool::new(dir.path()).with_history(Some(history.clone())),
        write: WriteTool::new(dir.path()).with_history(Some(history.clone())),
        undo: UndoTool::new(dir.path(), history),
        dir,
    }
}

impl Tools {
    async fn edit(&self, old: &str, new: &str) {
        let args: EditArgs = serde_json::from_value(serde_json::json!({
            "path": "a.txt", "edits": [{ "oldText": old, "newText": new }]
        }))
        .unwrap();
        assert!(!self.edit.execute(args).await.unwrap().is_error);
    }

    async fn write(&self, path: &str, content: &str) {
        let args = WriteArgs {
            path: path.into(),
            content: content.into(),
        };
        assert!(!self.write.execute(args).await.unwrap().is_error);
    }

    async fn undo(&self, path: &str) -> ToolResult {
        self.undo.run(UndoArgs { path: path.into() }).await.unwrap()
    }

    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.path().join(path)).ok()
    }
}

#[tokio::test]
async fn undo_steps_back_through_edits_and_writes() {
    let t = tools();
    std::fs::write(t.dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    t.edit("two", "2").await;
    t.write("a.txt", "rewritten\n").await;

    let first = t.undo("a.txt").await;
    assert_eq!(
        first.content,
        "Restored a.txt to its contents before the last change (1 earlier change(s) can still be undone)\n@@ 1-2\n1\tone\n2\t2"
    );
    assert_eq!(t.read("a.txt").as_deref(), Some("one\n2\n"));

    t.undo("a.txt").await;
    assert_eq!(t.read("a.txt").as_deref(), Some("one\ntwo\n"));

    let nothing = t.undo("a.txt").await;
    assert!(nothing.is_error);
    assert!(nothing.content.contains("no edit or write"));
}

#[tokio::test]
async fn undoing_a_created_file_deletes_it() {
    let t = tools();
    t.write("new.txt", "fresh\n").await;
    let result = t.undo("new.txt").await;
    assert_eq!(result.content, "Deleted new.txt, which write had created");
    assert_eq!(t.read("new.txt"), None);
}

#[tokio::test]
async fn undo_refuses_when_the_file_changed_after_the_tool_wrote_it() {
    let t = tools();
    std::fs::write(t.dir.path().join("a.txt"), "one\n").unwrap();
    t.edit("one", "1").await;
    std::fs::write(t.dir.path().join("a.txt"), "1\nuser line\n").unwrap();

    let refused = t.undo("a.txt").await;
    assert!(refused.is_error);
    assert!(refused.content.contains("changed after the last edit or write"));
    assert_eq!(t.read("a.txt").as_deref(), Some("1\nuser line\n"));
}

#[test]
fn history_keeps_only_the_newest_revisions_per_file() {
    let history = EditHistory::default();
    let path = Path::new("/ws/a.txt");
    for i in 0..(MAX_REVISIONS_PER_FILE + 5) {
        history.record(path, Some(vec![i as u8]), &[i as u8 + 1]);
    }
    let last = [(MAX_REVISIONS_PER_FILE + 5) as u8];
    let Undoing { before, .. } = history.peek_if_current(path, Some(&last)).unwrap();
    assert_eq!(before, Some(vec![(MAX_REVISIONS_PER_FILE + 4) as u8]));
    assert_eq!(history.pop(path), MAX_REVISIONS_PER_FILE - 1);
}

#[test]
fn history_evicts_the_globally_oldest_revision_across_files_past_its_byte_budget() {
    let history = EditHistory::default();
    let chunk = vec![0u8; MAX_HISTORY_BYTES / 3];
    let paths: Vec<PathBuf> = (0..7).map(|i| PathBuf::from(format!("/ws/f{i}.bin"))).collect();
    for path in &paths {
        history.record(path, None, &chunk);
    }
    let state = history.state();
    let total: usize = state.files.values().flatten().map(Revision::bytes).sum();
    assert!(total <= MAX_HISTORY_BYTES, "{total}");
    assert_eq!(state.files.len(), 3);
    assert!(paths[..4].iter().all(|path| !state.files.contains_key(path)));
    assert!(paths[4..].iter().all(|path| state.files.contains_key(path)));
}

#[test]
fn history_eviction_takes_an_old_revision_of_a_busy_file_before_a_newer_single_file() {
    let history = EditHistory::default();
    let chunk = vec![0u8; MAX_HISTORY_BYTES / 3];
    let (busy, other) = (Path::new("/ws/busy.bin"), Path::new("/ws/other.bin"));
    history.record(busy, None, &chunk);
    history.record(other, None, &chunk);
    history.record(busy, Some(chunk.clone()), &chunk);
    let state = history.state();
    assert_eq!(state.files.get(busy).map(Vec::len), Some(1));
    assert_eq!(state.files.get(other).map(Vec::len), Some(1));
}

#[tokio::test]
async fn undo_of_a_removed_file_says_so_and_keeps_the_history() {
    let t = tools();
    t.write("gone.txt", "x\n").await;
    std::fs::remove_file(t.dir.path().join("gone.txt")).unwrap();
    let result = t.undo("gone.txt").await;
    assert!(result.is_error);
    assert!(result.content.contains("no longer exists"), "{}", result.content);
    t.write("gone.txt", "x\n").await;
    assert!(
        t.undo("gone.txt")
            .await
            .content
            .contains("earlier change(s) can still be undone")
    );
}

/// False when the process can read a mode-000 file (root), so permission failures cannot be provoked.
#[cfg(unix)]
fn permissions_are_enforced() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("probe");
    std::fs::write(&file, "x").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let enforced = std::fs::read(&file).is_err();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    if !enforced {
        eprintln!("skipped: file permissions are not enforced for this user (root)");
    }
    enforced
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_restore_changes_nothing_and_keeps_the_revision() {
    use std::os::unix::fs::PermissionsExt;
    if !permissions_are_enforced() {
        return;
    }
    let t = tools();
    std::fs::write(t.dir.path().join("a.txt"), "one\n").unwrap();
    t.edit("one", "1").await;
    let mode = |m| std::fs::set_permissions(t.dir.path(), std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    let failed = t.undo("a.txt").await;
    mode(0o755);
    assert!(failed.is_error);
    assert!(failed.content.contains("nothing was changed"), "{}", failed.content);
    assert_eq!(t.read("a.txt").as_deref(), Some("1\n"));
    t.undo("a.txt").await;
    assert_eq!(t.read("a.txt").as_deref(), Some("one\n"));
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_existing_file_is_not_recorded_so_undo_cannot_delete_it() {
    use std::os::unix::fs::PermissionsExt;
    if !permissions_are_enforced() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    std::fs::write(&file, "keep").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let before = read_before(&file).await;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(before, None);
    assert_eq!(read_before(&dir.path().join("missing")).await, Some(None));
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_current_file_is_an_error_not_a_missing_one() {
    use std::os::unix::fs::PermissionsExt;
    if !permissions_are_enforced() {
        return;
    }
    let t = tools();
    std::fs::write(t.dir.path().join("a.txt"), "one\n").unwrap();
    t.edit("one", "1").await;
    let file = t.dir.path().join("a.txt");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let failed = t.undo("a.txt").await;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(failed.is_error);
    assert!(failed.content.starts_with("reading a.txt"), "{}", failed.content);
    assert!(!failed.content.contains("no longer exists"), "{}", failed.content);
    assert_eq!(t.read("a.txt").as_deref(), Some("1\n"));
}

#[tokio::test]
async fn undo_restores_a_non_utf8_file_without_a_diff_preview() {
    let t = tools();
    let binary = [0xff, 0xfe, 0x00, 0x80];
    std::fs::write(t.dir.path().join("b.bin"), binary).unwrap();
    t.write("b.bin", "text now\n").await;
    let result = t.undo("b.bin").await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(result.content, "Restored b.bin to its contents before the last change");
    assert_eq!(std::fs::read(t.dir.path().join("b.bin")).unwrap(), binary);
}

#[tokio::test]
async fn undo_rejects_a_path_outside_the_workspace() {
    let t = tools();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("x.txt");
    std::fs::write(&file, "keep").unwrap();
    let result = t.undo(file.to_str().unwrap()).await;
    assert!(result.is_error);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep");
    assert!(t.undo("../x.txt").await.is_error);
}

#[tokio::test]
async fn undoing_a_created_file_removes_the_empty_directories_write_created_but_no_others() {
    let t = tools();
    std::fs::create_dir(t.dir.path().join("old")).unwrap();
    t.write("old/new/deep/a.rs", "x").await;
    t.write("old/new/b.rs", "y").await;

    let first = t.undo("old/new/deep/a.rs").await;
    assert!(!first.is_error, "{}", first.content);
    assert!(first.content.contains("1 empty directory"), "{}", first.content);
    assert!(!t.dir.path().join("old/new/deep").exists());
    assert!(t.dir.path().join("old/new/b.rs").exists());

    t.write("old/c.rs", "z").await;
    assert!(!t.undo("old/c.rs").await.is_error);
    assert!(
        t.dir.path().join("old").is_dir(),
        "a directory that already existed stays"
    );
}
