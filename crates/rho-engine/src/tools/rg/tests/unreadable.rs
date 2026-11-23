use super::super::*;
use rho_test_support::TestDir;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn files_that_could_not_be_searched_are_reported_not_silently_skipped() {
    let dir = TestDir::new();
    std::fs::write(dir.join("a.txt"), "needle\n").unwrap();
    let locked = dir.join("b.txt");
    std::fs::write(&locked, "needle\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&locked).is_ok() {
        return;
    }
    let args = RgArgs {
        pattern: "needle".to_string(),
        path: None,
        file_type: None,
        hidden: None,
        output_mode: None,
        context: None,
        multiline: None,
        offset: None,
        limit: None,
    };

    let result = RgTool::new(&*dir).execute(args).await.unwrap();

    assert!(result.content.contains("a.txt"), "{}", result.content);
    assert!(
        result.content.contains("[1 file could not be searched; first: b.txt: "),
        "{}",
        result.content
    );
}
