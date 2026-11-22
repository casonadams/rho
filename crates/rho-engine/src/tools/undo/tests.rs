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
    let (before, _) = history.peek_if_current(path, Some(&last)).unwrap();
    assert_eq!(before, Some(vec![(MAX_REVISIONS_PER_FILE + 4) as u8]));
    assert_eq!(history.pop(path), MAX_REVISIONS_PER_FILE - 1);
}

#[test]
fn history_drops_the_oldest_revisions_past_its_byte_budget() {
    let history = EditHistory::default();
    let path = Path::new("/ws/big.bin");
    let chunk = vec![0u8; MAX_HISTORY_BYTES / 4];
    for _ in 0..4 {
        history.record(path, Some(chunk.clone()), &chunk);
    }
    let files = history.files();
    let kept = files[path].len();
    assert!((1..4).contains(&kept), "{kept}");
    assert!(files.values().flatten().map(Revision::bytes).sum::<usize>() <= MAX_HISTORY_BYTES);
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

#[cfg(unix)]
#[tokio::test]
async fn a_failed_restore_changes_nothing_and_keeps_the_revision() {
    use std::os::unix::fs::PermissionsExt;
    let t = tools();
    std::fs::write(t.dir.path().join("a.txt"), "one\n").unwrap();
    t.edit("one", "1").await;
    let mode = |m| std::fs::set_permissions(t.dir.path(), std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    let failed = t.undo("a.txt").await;
    mode(0o755);
    if failed.is_error {
        assert!(failed.content.contains("nothing was changed"), "{}", failed.content);
        assert_eq!(t.read("a.txt").as_deref(), Some("1\n"));
        t.undo("a.txt").await;
    }
    assert_eq!(t.read("a.txt").as_deref(), Some("one\n"));
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_existing_file_is_not_recorded_so_undo_cannot_delete_it() {
    use std::os::unix::fs::PermissionsExt;
    let path = tempfile::tempdir().unwrap();
    let file = path.path().join("secret.txt");
    std::fs::write(&file, "keep").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read(&file).is_ok();
    let before = read_before(&file).await;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    if !readable {
        assert_eq!(before, None);
    }
    assert_eq!(read_before(&path.path().join("missing")).await, Some(None));
}
