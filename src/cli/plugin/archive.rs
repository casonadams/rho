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

fn extract_from_tar_gz(data: &[u8], executable_name: &str) -> Result<Vec<u8>, ArchiveError> {
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);
    let entries = archive.entries().map_err(|e| ArchiveError::Corrupt(e.to_string()))?;

    let mut found_files: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
        if entry.header().entry_type().is_file() {
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
            found_files.push((file_name, content));
        }
    }

    if let Some((_, content)) = found_files.iter().find(|(name, _)| name == executable_name) {
        return Ok(content.clone());
    }

    let exe_name = format!("{executable_name}.exe");
    if let Some((_, content)) = found_files.iter().find(|(name, _)| name == &exe_name) {
        return Ok(content.clone());
    }

    let short_name = executable_name.strip_prefix("rho-plugin-").unwrap_or(executable_name);
    if let Some((_, content)) = found_files.iter().find(|(name, _)| name == short_name) {
        return Ok(content.clone());
    }

    if found_files.len() == 1 {
        return Ok(found_files.into_iter().next().unwrap().1);
    }

    Err(ArchiveError::MissingBinary {
        expected: executable_name.to_string(),
        found: found_files.into_iter().map(|(name, _)| name).collect(),
    })
}

#[cfg(test)]
#[path = "archive/tests.rs"]
mod tests;
