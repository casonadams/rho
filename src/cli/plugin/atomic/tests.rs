use super::*;

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
