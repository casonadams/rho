//! Where and how full-output logs are created: private to the current user, since the default
//! directory under the shared temp dir has a predictable name.

use std::fs::File;
use std::io;
use std::path::Path;

/// Creates `dir` with mode 0700 and refuses a symlink or a directory owned by someone else.
pub(super) fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is a symlink or owned by another user", dir.display()),
            ));
        }
        if meta.mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)
}

/// Creates a new log file readable only by the current user.
pub(super) fn create_log_file(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::accumulator::OutputAccumulator;
    use rho_test_support::TestDir;
    use std::os::unix::fs::{MetadataExt, symlink};

    fn log_of_twenty_lines(dir: &std::path::Path, line: &str) -> OutputAccumulator {
        let mut acc = OutputAccumulator::new().with_log_dir(dir);
        for _ in 0..20 {
            acc.append(format!("{line}\n").as_bytes());
        }
        acc.finish();
        acc
    }

    #[test]
    fn the_log_dir_and_files_are_private_to_the_user() {
        let root = TestDir::new();
        let logs = root.join("nested").join("rho-bash");
        let acc = log_of_twenty_lines(&logs, "line");

        let file = acc.snapshot().full_output_path.expect("log written");
        assert_eq!(std::fs::metadata(&logs).unwrap().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn an_existing_loose_log_dir_is_tightened() {
        let root = TestDir::new();
        let logs = root.join("rho-bash");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        log_of_twenty_lines(&logs, "line");

        assert_eq!(std::fs::metadata(&logs).unwrap().mode() & 0o777, 0o700);
    }

    #[test]
    fn a_symlinked_log_dir_is_refused_and_nothing_is_written_through_it() {
        let root = TestDir::new();
        let target = root.join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        let logs = root.join("rho-bash");
        symlink(&target, &logs).unwrap();

        let acc = log_of_twenty_lines(&logs, "line");

        assert!(acc.snapshot().full_output_path.is_none());
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn a_log_cut_at_the_size_limit_stays_valid_utf8() {
        let root = TestDir::new();
        let mut acc = OutputAccumulator::new()
            .with_log_dir(root.join("rho-bash"))
            .with_log_limit(10);
        for _ in 0..20 {
            acc.append("\u{e9}\n".as_bytes());
        }
        acc.finish();

        let file = acc.snapshot().full_output_path.expect("log written");
        let log = String::from_utf8(std::fs::read(file).unwrap()).expect("valid utf-8");
        assert!(
            log.starts_with("\u{e9}\n\u{e9}\n\u{e9}\n\n[log truncated at 10 bytes]"),
            "{log:?}"
        );
    }

    #[test]
    fn clipped_output_does_not_claim_a_log_when_none_could_be_created() {
        let root = TestDir::new();
        let blocker = root.join("file");
        std::fs::write(&blocker, "").unwrap();
        let mut acc = OutputAccumulator::new().with_log_dir(blocker.join("rho-bash"));
        acc.append(&vec![b'x'; crate::tools::truncate::DEFAULT_MAX_BYTES + 500]);
        acc.finish();

        let snapshot = acc.snapshot();
        assert!(snapshot.truncation.truncated && snapshot.full_output_path.is_none());
        assert!(!snapshot.formatted_text.contains("log:"), "{}", snapshot.formatted_text);
    }
}
