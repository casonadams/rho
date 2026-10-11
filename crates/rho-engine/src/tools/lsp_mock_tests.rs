use super::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn test_lsp_tool_operations_with_mock() {
    let (client_read, mut server_write) = tokio::io::duplex(4096);
    let (mut server_read, client_write) = tokio::io::duplex(4096);

    let temp_dir = tempfile::tempdir().unwrap();
    let client = Arc::new(LspClient::new(client_read, client_write, temp_dir.path().to_path_buf()));
    let tool = LspTool::with_client(temp_dir.path(), client.clone());

    tokio::spawn(async move {
        let mut buf = Vec::new();
        while let Ok(Some(frame)) = crate::lsp::client::read_frame(&mut server_read, &mut buf).await {
            let req: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let id = req.get("id").cloned();
            if let Some(id_val) = id {
                let result = match method {
                    "initialize" => serde_json::json!({ "capabilities": {} }),
                    "textDocument/definition" => serde_json::json!([{
                        "uri": "file:///workspace/src/lib.rs",
                        "range": {
                            "start": { "line": 9, "character": 4 },
                            "end": { "line": 9, "character": 12 }
                        }
                    }]),
                    "textDocument/references" => serde_json::json!([{
                        "uri": "file:///workspace/src/main.rs",
                        "range": {
                            "start": { "line": 15, "character": 8 },
                            "end": { "line": 15, "character": 16 }
                        }
                    }]),
                    "textDocument/documentSymbol" => serde_json::json!([{
                        "name": "MyStruct",
                        "kind": 23,
                        "range": {
                            "start": { "line": 2, "character": 0 },
                            "end": { "line": 10, "character": 1 }
                        }
                    }]),
                    "textDocument/rename" => serde_json::json!({
                        "changes": {
                            "file:///workspace/src/lib.rs": [{
                                "range": { "start": { "line": 9, "character": 4 }, "end": { "line": 9, "character": 12 } },
                                "newText": "Renamed"
                            }]
                        }
                    }),
                    _ => serde_json::Value::Null,
                };
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id_val,
                    "result": result
                });
                let out_frame = crate::lsp::client::encode_frame(&serde_json::to_vec(&resp).unwrap());
                let _ = server_write.write_all(&out_frame).await;
            }
        }
    });

    let def_res = tool
        .execute_lsp(LspArgs {
            operation: "definition".into(),
            path: Some("src/main.rs".into()),
            line: Some(5),
            character: Some(3),
            new_name: None,
        })
        .await
        .unwrap();
    assert!(!def_res.is_error);
    assert!(def_res.content.contains(":10:5"));

    let ref_res = tool
        .execute_lsp(LspArgs {
            operation: "references".into(),
            path: Some("src/lib.rs".into()),
            line: Some(10),
            character: Some(5),
            new_name: None,
        })
        .await
        .unwrap();
    assert!(!ref_res.is_error);
    assert!(ref_res.content.contains(":16:9"));

    let sym_res = tool
        .execute_lsp(LspArgs {
            operation: "symbols".into(),
            path: Some("src/lib.rs".into()),
            line: None,
            character: None,
            new_name: None,
        })
        .await
        .unwrap();
    assert!(!sym_res.is_error);
    assert!(sym_res.content.contains("Struct: MyStruct (lines 3-11)"));

    let ren_res = tool
        .execute_lsp(LspArgs {
            operation: "rename".into(),
            path: Some("src/lib.rs".into()),
            line: Some(10),
            character: Some(5),
            new_name: Some("Renamed".into()),
        })
        .await
        .unwrap();
    assert!(!ren_res.is_error);
    assert!(ren_res.content.contains("Proposed rename edits to 'Renamed'"));

    let diag_res = tool
        .execute_lsp(LspArgs {
            operation: "diagnostics".into(),
            path: None,
            line: None,
            character: None,
            new_name: None,
        })
        .await
        .unwrap();
    assert!(!diag_res.is_error);
    assert!(diag_res.content.contains("Workspace clean"));
}
