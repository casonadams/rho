//! Mountless copy-on-write directory tree cloning (APFS `clonefile(2)` on macOS,
//! `FICLONE` ioctl reflinks on Linux btrfs/XFS/bcachefs).
//!
//! Adapted from the MIT-licensed `pi-iso` crate (`oh-my-pi`). Both backends produce
//! ordinary files and directories with no kernel mounts to tear down.

use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneKind {
    Apfs,
    Reflink,
}

impl CloneKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Apfs => "APFS clonefile",
            Self::Reflink => "Linux reflink",
        }
    }
}

/// Attempts a copy-on-write clone of `src` into `dst`, omitting any top-level entries
/// listed in `skip_top_level` (typically `[OsStr::new(".git")]`).
///
/// On failure, the caller should remove `dst` and fall back to a regular checkout.
pub fn clone_tree(src: &Path, dst: &Path, skip_top_level: &[&OsStr]) -> Result<CloneKind, String> {
    #[cfg(target_os = "macos")]
    {
        clone_children(src, dst, skip_top_level, apfs::clone_entry).map(|()| CloneKind::Apfs)
    }
    #[cfg(target_os = "linux")]
    {
        clone_children(src, dst, skip_top_level, reflink::clone_entry).map(|()| CloneKind::Reflink)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (src, dst, skip_top_level);
        Err("CoW cloning is not supported on this platform".to_string())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn clone_children(
    src: &Path,
    dst: &Path,
    skip: &[&OsStr],
    clone_entry: fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("could not create {}: {e}", dst.display()))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("could not read {}: {e}", src.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry failed: {e}"))?;
        let name = entry.file_name();
        if skip.contains(&name.as_os_str()) {
            continue;
        }
        clone_entry(&entry.path(), &dst.join(&name))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod apfs {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const CLONE_NOFOLLOW: u32 = 0x0001;

    unsafe extern "C" {
        fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32) -> libc::c_int;
    }

    fn c_path(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| format!("path contains interior NUL: {}", path.display()))
    }

    pub(super) fn clone_entry(src: &Path, dst: &Path) -> Result<(), String> {
        let src_c = c_path(src)?;
        let dst_c = c_path(dst)?;
        let rc = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), CLONE_NOFOLLOW) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        Err(format!("clonefile {} -> {}: {err}", src.display(), dst.display()))
    }
}

#[cfg(target_os = "linux")]
mod reflink {
    use std::fs::{File, Metadata};
    use std::os::unix::fs::symlink;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    const FICLONE: libc::c_ulong = 0x4004_9409;

    fn ficlone_file(src: &Path, dst: &Path, meta: &Metadata) -> Result<(), String> {
        let src_file = File::open(src).map_err(|e| format!("open {}: {e}", src.display()))?;
        let dst_file = File::create(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
        let rc = unsafe { libc::ioctl(dst_file.as_raw_fd(), FICLONE as libc::Ioctl, src_file.as_raw_fd()) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            drop(dst_file);
            let _ = std::fs::remove_file(dst);
            return Err(format!("FICLONE {} -> {}: {err}", src.display(), dst.display()));
        }
        let _ = dst_file.set_permissions(meta.permissions());
        Ok(())
    }

    pub(super) fn clone_entry(src: &Path, dst: &Path) -> Result<(), String> {
        let meta = std::fs::symlink_metadata(src).map_err(|e| format!("stat {}: {e}", src.display()))?;
        let file_type = meta.file_type();
        if file_type.is_symlink() {
            let target = std::fs::read_link(src).map_err(|e| format!("read_link {}: {e}", src.display()))?;
            symlink(&target, dst).map_err(|e| format!("symlink {}: {e}", dst.display()))?;
            return Ok(());
        }
        if file_type.is_dir() {
            return super::clone_children(src, dst, &[], clone_entry);
        }
        if file_type.is_file() {
            ficlone_file(src, dst, &meta)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    #[test]
    fn clone_tree_skips_git_and_isolates_edits_when_supported() {
        let dir = TestDir::new();
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(src.join(".env"), "SECRET=orig\n").unwrap();
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("nested/file.txt"), "hello").unwrap();

        match clone_tree(&src, &dst, &[OsStr::new(".git")]) {
            Ok(_) => {
                assert!(!dst.join(".git").exists(), ".git must be skipped");
                assert_eq!(std::fs::read_to_string(dst.join(".env")).unwrap(), "SECRET=orig\n");
                assert_eq!(std::fs::read_to_string(dst.join("nested/file.txt")).unwrap(), "hello");
                std::fs::write(dst.join(".env"), "SECRET=mutated\n").unwrap();
                assert_eq!(
                    std::fs::read_to_string(src.join(".env")).unwrap(),
                    "SECRET=orig\n",
                    "CoW clone must isolate writes from source"
                );
            }
            Err(err) => {
                // Non-CoW filesystems (e.g. tmpfs / ext4) cleanly report an error for fallback.
                assert!(!err.is_empty());
            }
        }
    }
}
