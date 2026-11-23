use super::*;

fn temp_root() -> (crate::test_support::TestDir, PathBuf) {
    let dir = crate::test_support::temp_dir();
    let root = dir.canonicalize().unwrap();
    (dir, root)
}

#[test]
fn resolves_paths_from_fixed_root() {
    let (_dir, root) = temp_root();
    let workspace = Workspace::new(&root);
    let cases = [
        ("src/lib.rs", Some(root.join("src/lib.rs"))),
        (" ", None),
        ("\"src/lib.rs\"", Some(root.join("src/lib.rs"))),
        (
            "\"/no_such_root_dir/keys\"",
            Some(PathBuf::from("/no_such_root_dir/keys")),
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(workspace.resolve(input), expected, "{input:?}");
    }
    let resolved = workspace.resolve("../outside.txt").unwrap();
    assert_eq!(resolved, root.parent().unwrap().join("outside.txt"));
    assert!(!resolved.starts_with(workspace.root()));
}

#[test]
fn resolves_home_paths_when_env_present() {
    let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
    let workspace = Workspace::new(&root);
    if let Ok(home) = std::env::var("HOME") {
        assert_eq!(workspace.resolve("~"), Some(PathBuf::from(&home)));
        assert_eq!(workspace.resolve("~/test"), Some(PathBuf::from(&home).join("test")));
    }
}

#[cfg(unix)]
#[test]
fn new_file_under_symlinked_parent_resolves_to_link_target() {
    let (_root_dir, root) = temp_root();
    let (_outside_dir, outside) = temp_root();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    let workspace = Workspace::new(&root);
    assert_eq!(workspace.resolve("link/new.txt"), Some(outside.join("new.txt")));
}
