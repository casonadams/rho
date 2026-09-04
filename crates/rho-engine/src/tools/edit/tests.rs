use super::*;

#[tokio::test]
async fn test_edit_unique_replacement() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    tokio::fs::write(&file_path, "fn hello() {\n    println!(\"world\");\n}\n")
        .await
        .unwrap();

    let tool = EditTool::new(&temp_dir);
    let res = tool
        .execute(EditArgs {
            path: file_path.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "println!(\"world\");".to_string(),
                new_text: "println!(\"rust\");".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert_eq!(res.metadata, Some(serde_json::json!({ "line_numbers": [2] })));
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "fn hello() {\n    println!(\"rust\");\n}\n");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_edit_crlf_file_with_lf_old_text() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("crlf_sample.txt");
    tokio::fs::write(&file_path, "line 1\r\nline 2\r\nline 3\r\n")
        .await
        .unwrap();

    let tool = EditTool::new(&temp_dir);
    // Send standard LF newlines in old_text and new_text
    let res = tool
        .execute(EditArgs {
            path: file_path.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "line 2\n".to_string(),
                new_text: "line 2 modified\n".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    // Verify file content was replaced and still maintains CRLF
    assert_eq!(updated, "line 1\r\nline 2 modified\r\nline 3\r\n");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_edit_duplicate_match_fails_atomically() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    let initial_content = "foo bar foo baz\n";
    tokio::fs::write(&file_path, initial_content).await.unwrap();

    let tool = EditTool::new(&temp_dir);
    let res = tool
        .execute(EditArgs {
            path: file_path.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "foo".to_string(),
                new_text: "qux".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("found 2 times"));
    assert!(res.content.contains("Provide more surrounding context"));
    let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(disk, initial_content); // Unchanged

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_edit_missing_match_shows_whitespace_hint() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    let initial_content = "    fn calculate() {\n        let x = 1;\n    }\n";
    tokio::fs::write(&file_path, initial_content).await.unwrap();

    let tool = EditTool::new(&temp_dir);
    let res = tool
        .execute(EditArgs {
            path: file_path.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "  fn calculate() {\n    let x = 1;\n  }".to_string(),
                new_text: "  fn calculate() {\n    let x = 2;\n  }".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("oldText not found"));
    assert!(res.content.contains("different whitespace or indentation"));
    let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(disk, initial_content);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_edit_missing_match_fails_atomically() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    let initial_content = "hello world\n";
    tokio::fs::write(&file_path, initial_content).await.unwrap();

    let tool = EditTool::new(&temp_dir);
    let res = tool
        .execute(EditArgs {
            path: file_path.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "not_present".to_string(),
                new_text: "replacement".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("oldText not found"));
    assert!(!res.content.contains("different whitespace or indentation"));
    let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(disk, initial_content);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_edit_rejects_directory() {
    let temp_dir = std::env::temp_dir().join(format!("edit_test_{}", uuid::Uuid::new_v4()));
    let sub = temp_dir.join("subdir");
    tokio::fs::create_dir_all(&sub).await.unwrap();

    let tool = EditTool::new(&temp_dir);
    let res = tool
        .execute(EditArgs {
            path: sub.to_str().unwrap().to_string(),
            edits: vec![EditReplacement {
                old_text: "a".to_string(),
                new_text: "b".to_string(),
            }],
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("target path is a directory"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
