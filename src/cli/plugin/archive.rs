use std::io::Read;

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

#[cfg(test)]
#[path = "archive/tests.rs"]
mod tests;
