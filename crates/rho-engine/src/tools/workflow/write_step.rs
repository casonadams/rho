use rho_harness_core::fs::write_atomic_async;
use std::path::{Component, Path, PathBuf};

const OUTPUT_ROOT: &str = ".rho";
const FILE_MODE: u32 = 0o644;

/// Workflows run in a throwaway worktree, so output files are written under the project's `.rho/`
/// instead, which survives the run.
fn output_path(project_dir: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative.trim());
    let mut parts = path.components();
    let confined = parts.next() == Some(Component::Normal(OUTPUT_ROOT.as_ref()))
        && parts.clone().next().is_some()
        && parts.all(|c| matches!(c, Component::Normal(_)));
    if confined {
        Ok(project_dir.join(path))
    } else {
        Err(format!(
            "write path must be a relative path inside {OUTPUT_ROOT}/ (got '{relative}')"
        ))
    }
}

pub(super) async fn write_output(project_dir: &Path, relative: &str, content: String) -> Result<String, String> {
    let target = output_path(project_dir, relative)?;
    let bytes = content.len();
    write_atomic_async(&target, content.into_bytes(), FILE_MODE)
        .await
        .map_err(|e| format!("cannot write {relative}: {e}"))?;
    Ok(format!("{bytes} bytes to {}", relative.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn accepts_only_relative_paths_inside_the_output_root() {
        let root = Path::new("/p");
        for ok in [".rho/a.md", ".rho/audit/x/report.md", " .rho/a.md "] {
            assert!(output_path(root, ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".rho",
            "a.md",
            "src/lib.rs",
            "/etc/passwd",
            ".rho/../x",
            "../.rho/x",
            ".rho/./../../x",
            ".rhoo/x",
        ] {
            assert!(output_path(root, bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn writes_content_creating_parent_directories() {
        let dir = TestDir::new();
        let root: &Path = &dir;
        let detail = write_output(root, ".rho/audit/run/report.md", "# hi".into())
            .await
            .unwrap();
        assert_eq!(detail, "4 bytes to .rho/audit/run/report.md");
        let text = std::fs::read_to_string(dir.join(".rho/audit/run/report.md")).unwrap();
        assert_eq!(text, "# hi");
    }

    #[tokio::test]
    async fn rejects_escaping_paths_without_writing() {
        let dir = TestDir::new();
        let root: &Path = &dir;
        let err = write_output(root, ".rho/../escape.txt", "x".into()).await.unwrap_err();
        assert!(err.contains("inside .rho/"));
        assert!(!dir.join("escape.txt").exists());
    }
}
