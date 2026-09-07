//! Archive decompression and atomic binary persistence for plugins.

use std::io::Read;
use std::path::Path;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ArchiveError {
    #[error("archive does not contain executable '{expected}' (found: {found:?})")]
    MissingBinary { expected: String, found: Vec<String> },
    #[error("unsupported archive format '{0}'")]
    UnsupportedFormat(String),
    #[error("decompression or archive read failed: {0}")]
    Corrupt(String),
}

pub fn extract_binary(asset_name: &str, data: &[u8], executable_name: &str) -> Result<Vec<u8>, ArchiveError> {
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        extract_from_tar_gz(data, executable_name)
    } else if is_unsupported_archive(asset_name) {
        Err(ArchiveError::UnsupportedFormat(asset_name.to_string()))
    } else {
        Ok(data.to_vec())
    }
}

fn is_unsupported_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".zip") || lower.ends_with(".tar.xz") || lower.ends_with(".tar.bz2") || lower.ends_with(".7z")
}

fn find_matching_executable<'a>(files: &'a [(String, Vec<u8>)], executable_name: &str) -> Option<&'a [u8]> {
    let exe_name = format!("{executable_name}.exe");
    let short_name = executable_name.strip_prefix("rho-plugin-").unwrap_or(executable_name);
    for target in [executable_name, &exe_name, short_name] {
        if let Some((_, content)) = files.iter().find(|(name, _)| name == target) {
            return Some(content);
        }
    }
    if files.len() == 1 {
        return Some(&files[0].1);
    }
    None
}

fn read_archive_entry<R: std::io::Read>(mut entry: tar::Entry<R>) -> Result<Option<(String, Vec<u8>)>, ArchiveError> {
    if !entry.header().entry_type().is_file() {
        return Ok(None);
    }
    let path = entry.path().map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();
    let mut content = Vec::new();
    entry
        .read_to_end(&mut content)
        .map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
    Ok(Some((file_name, content)))
}

fn extract_from_tar_gz(data: &[u8], executable_name: &str) -> Result<Vec<u8>, ArchiveError> {
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);
    let entries = archive.entries().map_err(|e| ArchiveError::Corrupt(e.to_string()))?;

    let mut found_files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
        if let Some(file) = read_archive_entry(entry)? {
            found_files.push(file);
        }
    }

    if let Some(content) = find_matching_executable(&found_files, executable_name) {
        return Ok(content.to_vec());
    }

    Err(ArchiveError::MissingBinary {
        expected: executable_name.to_string(),
        found: found_files.into_iter().map(|(name, _)| name).collect(),
    })
}

fn make_temp_path(parent: &Path, file_name: &str) -> std::path::PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(".{file_name}.tmp.{}.{now}", std::process::id()))
}

struct Guard<'a>(&'a Path);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

pub fn write_binary_atomically(dest_path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = dest_path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing parent directory"))?;
    std::fs::create_dir_all(parent)?;

    let file_name = dest_path.file_name().and_then(|f| f.to_str()).unwrap_or("binary");
    let tmp_path = make_temp_path(parent, file_name);
    let guard = Guard(&tmp_path);

    std::fs::write(&tmp_path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(windows)]
    if dest_path.exists() {
        let _ = std::fs::remove_file(dest_path);
    }

    std::fs::rename(&tmp_path, dest_path)?;
    std::mem::forget(guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut tar = tar::Builder::new(&mut gz);
            for (name, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_path(name).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append(&header, *content).unwrap();
            }
            tar.finish().unwrap();
        }
        gz.finish().unwrap()
    }

    #[test]
    fn test_extract_exact_binary_match() {
        let tar_gz = create_tar_gz(&[("rho-plugin-foo", b"elf-binary-data")]);
        let extracted = extract_binary("plugin.tar.gz", &tar_gz, "rho-plugin-foo").unwrap();
        assert_eq!(extracted, b"elf-binary-data");
    }

    #[test]
    fn test_extract_nested_binary_in_subfolder() {
        let tar_gz = create_tar_gz(&[
            ("bundle/README.md", b"docs"),
            ("bundle/bin/my-plugin", b"nested-binary"),
        ]);
        let extracted = extract_binary("release.tgz", &tar_gz, "my-plugin").unwrap();
        assert_eq!(extracted, b"nested-binary");
    }

    #[test]
    fn test_extract_windows_exe_binary() {
        let tar_gz = create_tar_gz(&[("my-plugin.exe", b"windows-exe")]);
        let extracted = extract_binary("release.tar.gz", &tar_gz, "my-plugin").unwrap();
        assert_eq!(extracted, b"windows-exe");
    }

    #[test]
    fn test_extract_short_name_binary() {
        let tar_gz = create_tar_gz(&[("foo", b"short-name-binary")]);
        let extracted = extract_binary("release.tar.gz", &tar_gz, "rho-plugin-foo").unwrap();
        assert_eq!(extracted, b"short-name-binary");
    }

    #[test]
    fn test_extract_single_file_fallback() {
        let tar_gz = create_tar_gz(&[("unknown-name", b"only-file")]);
        let extracted = extract_binary("release.tar.gz", &tar_gz, "desired-name").unwrap();
        assert_eq!(extracted, b"only-file");
    }

    #[test]
    fn test_extract_missing_binary_error() {
        let tar_gz = create_tar_gz(&[("file1.txt", b"one"), ("file2.txt", b"two")]);
        let err = extract_binary("release.tar.gz", &tar_gz, "desired-bin").unwrap_err();
        match err {
            ArchiveError::MissingBinary { expected, found } => {
                assert_eq!(expected, "desired-bin");
                assert_eq!(found, vec!["file1.txt", "file2.txt"]);
            }
            other => panic!("expected MissingBinary, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_unsupported_archive_format() {
        let err = extract_binary("plugin.zip", b"fake-zip", "plugin").unwrap_err();
        assert!(matches!(err, ArchiveError::UnsupportedFormat(_)));
    }

    #[test]
    fn test_extract_raw_binary() {
        let raw = b"raw-binary-content";
        let extracted = extract_binary("rho-plugin-foo-x86_64", raw, "rho-plugin-foo").unwrap();
        assert_eq!(extracted, raw);
    }

    #[test]
    fn test_extract_corrupt_tar_gz() {
        let corrupt = b"not-a-tar-gz";
        let err = extract_binary("corrupt.tar.gz", corrupt, "plugin").unwrap_err();
        assert!(matches!(err, ArchiveError::Corrupt(_)));
    }

    #[test]
    fn test_write_binary_atomically_creates_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dest = temp_dir.path().join("sub").join("binary");

        write_binary_atomically(&dest, b"binary contents").unwrap();

        assert!(dest.is_file());
        assert_eq!(std::fs::read(&dest).unwrap(), b"binary contents");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&dest).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn test_write_binary_atomically_overwrites_existing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dest = temp_dir.path().join("existing");
        std::fs::write(&dest, b"old").unwrap();

        write_binary_atomically(&dest, b"new").unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }
}
