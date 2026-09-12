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
    #[error("failed to write binary: {0}")]
    Io(String),
}

pub fn extract_binary(asset_name: &str, data: &[u8], executable_name: &str) -> Result<Vec<u8>, ArchiveError> {
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        extract_from_tar_gz(data, executable_name)
    } else if asset_name.ends_with(".zip") || asset_name.ends_with(".tar.xz") || asset_name.ends_with(".tar.bz2") {
        Err(ArchiveError::UnsupportedFormat(asset_name.to_string()))
    } else {
        Ok(data.to_vec())
    }
}

fn find_matching_executable<'a>(files: &'a [(String, Vec<u8>)], executable_name: &str) -> Option<&'a [u8]> {
    let exe_name = format!("{executable_name}.exe");
    for target in [executable_name, &exe_name] {
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
        found: found_files.into_iter().map(|(n, _)| n).collect(),
    })
}

pub fn write_binary_atomically(dest: &Path, content: &[u8]) -> Result<(), ArchiveError> {
    let dir = dest
        .parent()
        .ok_or_else(|| ArchiveError::Io(format!("no parent directory for {}", dest.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| ArchiveError::Io(e.to_string()))?;

    let tmp_name = format!(".rho_self_update_{}", uuid::Uuid::new_v4());
    let tmp_path = dir.join(tmp_name);

    std::fs::write(&tmp_path, content).map_err(|e| ArchiveError::Io(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp_path)
            .map_err(|e| ArchiveError::Io(e.to_string()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_path, perms).map_err(|e| ArchiveError::Io(e.to_string()))?;
    }

    std::fs::rename(&tmp_path, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        ArchiveError::Io(e.to_string())
    })
}
