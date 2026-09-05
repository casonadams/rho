use std::path::Path;

pub fn write_binary_atomically(dest_path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = dest_path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing parent directory"))?;
    std::fs::create_dir_all(parent)?;

    let file_name = dest_path.file_name().and_then(|f| f.to_str()).unwrap_or("binary");
    let tmp_path = parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    struct Guard<'a>(&'a Path);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
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
#[path = "atomic/tests.rs"]
mod tests;
