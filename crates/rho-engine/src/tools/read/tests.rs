use super::*;

#[tokio::test]
async fn test_read_tool_happy_path() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    tokio::fs::write(&file_path, "line1\nline2\nline3\n").await.unwrap();

    let tool = ReadTool::new(&temp_dir);
    let res = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: Some(1),
            limit: Some(2),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("line1"));
    assert!(res.content.contains("line2"));
    assert!(!res.content.contains("line3"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_read_truncates_at_byte_limit() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("large.txt");
    tokio::fs::write(&file_path, "x".repeat(MAX_READ_BYTES * 2))
        .await
        .unwrap();

    let result = ReadTool::new(&temp_dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("Truncated at 50 KB limit"));
    assert!(result.content.len() <= MAX_READ_BYTES + 200);
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_read_missing_file() {
    let tool = ReadTool::new(std::env::temp_dir());
    let res = tool
        .execute(ReadArgs {
            path: "nonexistent_file_xyz_123.txt".to_string(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("File not found"));
}

#[tokio::test]
async fn test_read_builtin_embedded_skill() {
    let tool = ReadTool::new(std::env::temp_dir());
    let res = tool
        .execute(ReadArgs {
            path: "rho://skills/create-plugin".to_string(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("Creating an MCP Tool Plugin for `rho`"));
    assert!(res.content.contains("Model Context Protocol"));
}
