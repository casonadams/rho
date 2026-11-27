use std::ops::Deref;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Sets one process environment variable until dropped. The lock serializes environment changes
/// across parallel tests, so tests that read the variable must hold it too.
#[derive(Debug)]
pub struct EnvVarGuard {
    name: &'static str,
    _lock: MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    pub fn set(name: &'static str, value: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        // SAFETY: every environment write in these tests happens under ENV_LOCK.
        unsafe { std::env::set_var(name, value) };
        Self { name, _lock: lock }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: ENV_LOCK is still held while this guard drops.
        unsafe { std::env::remove_var(self.name) };
    }
}

/// A scratch directory removed on drop, including when the owning test panics.
#[derive(Debug)]
pub struct TestDir(tempfile::TempDir);

impl TestDir {
    pub fn new() -> Self {
        Self(tempfile::tempdir().expect("create test directory"))
    }

    pub fn with_prefix(prefix: &str) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(prefix)
                .tempdir()
                .expect("create test directory"),
        )
    }
}

impl Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl PartialEq<TestDir> for std::path::PathBuf {
    fn eq(&self, other: &TestDir) -> bool {
        self.as_path() == other.0.path()
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}

/// An empty file inside its own scratch directory, so sibling files such as `<file>.lock` are removed with it.
#[derive(Debug)]
pub struct TestFile {
    _dir: TestDir,
    path: std::path::PathBuf,
}

impl TestFile {
    pub fn new() -> Self {
        let dir = TestDir::new();
        let path = dir.join("file");
        std::fs::write(&path, b"").expect("create test file");
        Self { _dir: dir, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for TestFile {
    fn default() -> Self {
        Self::new()
    }
}

/// Removes the file at the wrapped path on drop, including when the owning test panics.
#[derive(Debug)]
pub struct RemoveOnDrop(pub std::path::PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_exists_while_held_and_is_removed_on_drop() {
        let dir = TestDir::with_prefix("rho-test-support-");
        let path = dir.to_path_buf();
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn env_var_guard_sets_the_variable_and_removes_it_on_drop() {
        const NAME: &str = "RHO_TEST_SUPPORT_ENV_GUARD";
        {
            let _guard = EnvVarGuard::set(NAME, "held");
            assert_eq!(std::env::var(NAME).as_deref(), Ok("held"));
        }
        assert!(std::env::var(NAME).is_err());
    }

    #[test]
    fn directory_is_removed_when_the_owner_panics() {
        let leaked = std::sync::Mutex::new(None);
        let result = std::panic::catch_unwind(|| {
            let dir = TestDir::new();
            *leaked.lock().unwrap() = Some(dir.to_path_buf());
            panic!("assertion failed");
        });
        assert!(result.is_err());
        assert!(!leaked.lock().unwrap().take().unwrap().exists());
    }
}
