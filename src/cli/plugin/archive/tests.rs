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
