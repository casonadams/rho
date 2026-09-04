use super::*;

#[tokio::test]
async fn rejects_excluded_targets_before_writing() {
    let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
    let excluded = temp_dir.join("rho");
    tokio::fs::create_dir_all(&excluded).await.unwrap();
    let path = excluded.join("config.toml");
    let tool = WriteTool::with_exclusions(&temp_dir, [&excluded]);
    let result = tool
        .execute(WriteArgs {
            path: path.to_string_lossy().into_owned(),
            content: "secret = true".to_string(),
        })
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(!path.exists());
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_write_tool() {
    let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
    let tool = WriteTool::new(&temp_dir);
    let file_path = temp_dir.join("sub/nested/file.txt");
    let res = tool
        .execute(WriteArgs {
            path: file_path.to_str().unwrap().to_string(),
            content: "hello world\nsecond line\n".to_string(),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("Successfully wrote"));
    let read_back = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(read_back, "hello world\nsecond line\n");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_write_rejects_directory_target() {
    let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
    let sub = temp_dir.join("subdir");
    tokio::fs::create_dir_all(&sub).await.unwrap();
    let tool = WriteTool::new(&temp_dir);
    let res = tool
        .execute(WriteArgs {
            path: sub.to_str().unwrap().to_string(),
            content: "payload".to_string(),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("target path is a directory"));
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_preserves_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = std::env::temp_dir().join(format!("write_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let script = temp_dir.join("script.sh");
    tokio::fs::write(&script, "#!/bin/sh\necho old\n").await.unwrap();
    tokio::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .await
        .unwrap();

    let tool = WriteTool::new(&temp_dir);
    let res = tool
        .execute(WriteArgs {
            path: script.to_str().unwrap().to_string(),
            content: "#!/bin/sh\necho new\n".to_string(),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    let perms = tokio::fs::metadata(&script).await.unwrap().permissions();
    assert_eq!(perms.mode() & 0o777, 0o755);
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
