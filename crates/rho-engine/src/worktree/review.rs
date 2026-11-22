//! The worktree side of the code review: the patch to review and the checkout it is compared with.

use super::{Worktree, git};
use crate::review::{Corpus, Lang, Review, is_ignored_path, review};
use std::path::PathBuf;

const MAX_FILES: usize = 4000;
const MAX_TOTAL_BYTES: u64 = 24 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Context lines requested so the review sees whole files, not just hunks.
const WHOLE_FILE_CONTEXT: &str = "-U100000";

/// The files of a checkout, honoring ignore files and skipping vendored, generated, and huge ones.
pub(crate) struct CheckoutCorpus {
    root: PathBuf,
}

impl Corpus for CheckoutCorpus {
    fn files(&self, family: Lang) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut total = 0u64;
        for entry in ignore::WalkBuilder::new(&self.root)
            .require_git(false)
            .build()
            .flatten()
        {
            let Some(rel) = entry
                .path()
                .strip_prefix(&self.root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
            else {
                continue;
            };
            let size = entry.metadata().map_or(u64::MAX, |m| m.len());
            if !entry.file_type().is_some_and(|t| t.is_file()) || size > MAX_FILE_BYTES {
                continue;
            }
            if Lang::from_path(&rel) != family || is_ignored_path(&rel) {
                continue;
            }
            total += size;
            if found.len() >= MAX_FILES || total > MAX_TOTAL_BYTES {
                break;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                found.push((rel, text));
            }
        }
        found
    }
}

impl Worktree {
    /// Commits the worktree and reviews what it added since it started.
    pub async fn review(&self) -> Result<Review, String> {
        let head = self.commit().await?;
        self.review_head(&head).await
    }

    /// Reviews the changes between the worktree's base and its commit `head`.
    pub async fn review_head(&self, head: &str) -> Result<Review, String> {
        let args = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            WHOLE_FILE_CONTEXT,
            &self.base,
            head,
        ];
        let patch = git::run(&self.path, &args, &[]).await?;
        let corpus = CheckoutCorpus {
            root: self.path.clone(),
        };
        tokio::task::spawn_blocking(move || review(&String::from_utf8_lossy(&patch), &corpus))
            .await
            .map_err(|e| format!("review failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::WorktreeStore;
    use super::super::git::test_repo::{create, write};
    use super::*;
    use rho_test_support::TestDir;

    const SHARED: &str = "fn shared_block(items: &[String]) -> usize {\n    let mut running_total = 0;\n    for current_item in items {\n        running_total += current_item.len();\n        if current_item.is_empty() {\n            running_total += 1;\n        }\n    }\n    let average_length = running_total / items.len().max(1);\n    running_total + average_length + items.len()\n}\n";

    #[test]
    fn the_corpus_lists_files_of_one_family_without_ignored_or_vendored_ones() {
        let dir = TestDir::new();
        let root = dir.to_path_buf();
        for (path, body) in [
            ("a.rs", "fn a() {}"),
            ("b.py", "x = 1"),
            ("node_modules/x/c.rs", "fn c() {}"),
            ("ignored/d.rs", "fn d() {}"),
            (".gitignore", "ignored/\n"),
        ] {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, body).unwrap();
        }
        let files = CheckoutCorpus { root }.files(Lang::Rust);
        assert_eq!(files, [("a.rs".to_string(), "fn a() {}".to_string())]);
    }

    #[tokio::test]
    async fn a_worktree_review_reports_what_the_change_added_and_nothing_else() {
        let (dir, root) = create(&[("src/shared.rs", SHARED), ("src/lib.rs", "pub fn old() {}\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let wt = store.create(&root, "subagent fix", None).await.unwrap().unwrap();
        let copied = format!("{SHARED}\n#[allow(dead_code)]\nfn unused() {{\n    dbg!(1);\n}}\n");
        write(&wt.path, "src/copy.rs", &copied.replace("shared_block", "copied_block"));

        let review = wt.review().await.unwrap();

        let found: Vec<(String, usize, &str)> = review
            .findings
            .iter()
            .map(|f| (f.path.clone(), f.line, f.rule))
            .collect();
        assert_eq!(
            found,
            [
                ("src/copy.rs".to_string(), 15, "debug-leftover"),
                ("src/copy.rs".to_string(), 2, "duplicate-code"),
                ("src/copy.rs".to_string(), 13, "lint-suppression"),
            ]
        );
        assert_eq!(review.counts(), (1, 2));
        assert!(review.findings[1].message.contains("src/shared.rs:2 (7 lines)"));
    }

    #[tokio::test]
    async fn duplicated_blocks_name_the_existing_file_and_clean_changes_have_no_findings() {
        let (dir, root) = create(&[("src/shared.rs", SHARED)]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let wt = store.create(&root, "subagent dup", None).await.unwrap().unwrap();
        write(&wt.path, "src/copy.rs", &format!("// copied\n{SHARED}"));

        let text = wt.review().await.unwrap().render(30);

        assert!(
            text.contains("src/copy.rs:2-11 [duplicate-code] duplicates src/shared.rs:1 (8 lines)"),
            "{text}"
        );
        let clean = store.create(&root, "subagent clean", None).await.unwrap().unwrap();
        write(&clean.path, "src/new.rs", "pub fn fine() -> u8 {\n    1\n}\n");
        assert!(
            clean
                .review()
                .await
                .unwrap()
                .render(30)
                .starts_with("Review: no findings")
        );
    }
}
