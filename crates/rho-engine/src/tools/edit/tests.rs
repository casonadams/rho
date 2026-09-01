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
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "fn hello() {\n    println!(\"rust\");\n}\n");

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
    let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(disk, initial_content); // Unchanged

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
    let disk = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(disk, initial_content);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
