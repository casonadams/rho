use super::*;

#[test]
fn test_todo_lifecycle_create_update_list_get_delete_clear() {
    let store = TodoStore::new();

    // 1. Create
    let res = store
        .create(TodoCreateParams {
            subject: "Task 1".to_string(),
            description: Some("Description 1".to_string()),
            status: Some(TaskStatus::Pending),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(res, "Created #1: Task 1 (pending)");

    let res2 = store
        .create(TodoCreateParams {
            subject: "Task 2".to_string(),
            status: Some(TaskStatus::Pending),
            blocked_by: Some(vec![1]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(res2, "Created #2: Task 2 (pending)");

    // 2. List
    let list_out = store.list(None, false);
    assert!(list_out.contains("#1: [pending] Task 1"));
    assert!(list_out.contains("#2: [pending] Task 2 (blocked by #1)"));

    // 3. Update status and activeForm
    let upd_res = store
        .update(TodoUpdateParams {
            id: 1,
            status: Some(TaskStatus::InProgress),
            active_form: Some("doing task 1".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(upd_res, "Updated #1 (pending → in_progress)");

    let list_out2 = store.list(None, false);
    assert!(list_out2.contains("#1: [in_progress] Task 1 (doing task 1)"));

    // 4. Get
    let get_out = store.get(1).unwrap();
    assert!(get_out.contains("Subject: Task 1"));
    assert!(get_out.contains("Status: in_progress"));
    assert!(get_out.contains("Active form: doing task 1"));

    // 5. Complete task 1
    let upd_res2 = store
        .update(TodoUpdateParams {
            id: 1,
            status: Some(TaskStatus::Completed),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(upd_res2, "Updated #1 (in_progress → completed)");

    // 6. Delete task 2
    let del_res = store.delete(2).unwrap();
    assert_eq!(del_res, "Deleted #2: Task 2");

    let list_out3 = store.list(None, false);
    assert!(!list_out3.contains("Task 2"));
    let list_out_deleted = store.list(None, true);
    assert!(list_out_deleted.contains("Task 2"));

    // 7. Clear
    let clear_res = store.clear();
    assert_eq!(clear_res, "Cleared all tasks.");
    assert_eq!(store.list(None, false), "No tasks found.");
}

#[test]
fn test_cycle_detection() {
    let store = TodoStore::new();
    store
        .create(TodoCreateParams {
            subject: "A".to_string(),
            ..Default::default()
        })
        .unwrap();
    store
        .create(TodoCreateParams {
            subject: "B".to_string(),
            blocked_by: Some(vec![1]),
            ..Default::default()
        })
        .unwrap();

    // Adding 2 as blockedBy for 1 would cause cycle 1 -> 2 -> 1
    let err = store
        .update(TodoUpdateParams {
            id: 1,
            add_blocked_by: Some(vec![2]),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.contains("Circular dependency"));

    // Task 1 should not have been mutated
    let task1 = store.get(1).unwrap();
    assert!(!task1.contains("blocked by"));
}

#[tokio::test]
async fn test_todo_schema_compilation_and_validation() {
    let schema_val = crate::tools::types::generated_schema::<TodoArgs>();
    let compiled = rho_sdk::schema::CompiledSchema::compile(&schema_val).expect("Schema must compile");

    // Assert schema has no $defs, $ref, or $schema left over
    assert!(schema_val.get("$defs").is_none());
    assert!(schema_val.get("definitions").is_none());
    assert!(schema_val.get("$schema").is_none());

    let valid_cases = vec![
        ("list plain", serde_json::json!({"action": "list"})),
        (
            "create with subject",
            serde_json::json!({"action": "create", "subject": "test"}),
        ),
        (
            "create with description",
            serde_json::json!({"action": "create", "subject": "test", "description": "foo"}),
        ),
        (
            "create with empty blockedBy",
            serde_json::json!({"action": "create", "subject": "test", "blockedBy": []}),
        ),
        (
            "create with snake_case active_form",
            serde_json::json!({"action": "create", "subject": "test", "active_form": "doing things"}),
        ),
        (
            "create with snake_case blocked_by",
            serde_json::json!({"action": "create", "subject": "test", "blocked_by": []}),
        ),
        (
            "update with id and status",
            serde_json::json!({"action": "update", "id": 1, "status": "in_progress"}),
        ),
        (
            "create with status pending",
            serde_json::json!({"action": "create", "subject": "test", "status": "pending"}),
        ),
        (
            "list with status pending",
            serde_json::json!({"action": "list", "status": "pending"}),
        ),
        (
            "list with snake include_deleted",
            serde_json::json!({"action": "list", "include_deleted": false}),
        ),
        (
            "list with camel includeDeleted",
            serde_json::json!({"action": "list", "includeDeleted": false}),
        ),
        ("get with int id", serde_json::json!({"action": "get", "id": 1})),
        ("delete with int id", serde_json::json!({"action": "delete", "id": 1})),
        ("clear plain", serde_json::json!({"action": "clear"})),
    ];

    for (label, case) in valid_cases {
        assert!(
            compiled.validate(&case).is_ok(),
            "Schema validation failed for '{label}': {case}"
        );
        assert!(
            serde_json::from_value::<TodoArgs>(case.clone()).is_ok(),
            "Serde deserialization failed for '{label}': {case}"
        );
    }

    // Additional flexible serde deserialization cases
    let serde_flexible_cases = vec![
        (
            "update with string id",
            serde_json::json!({"action": "update", "id": "1", "status": "in_progress"}),
        ),
        ("get with string id", serde_json::json!({"action": "get", "id": "1"})),
        (
            "delete with string id",
            serde_json::json!({"action": "delete", "id": "1"}),
        ),
        (
            "capitalized action",
            serde_json::json!({"action": "Create", "subject": "test"}),
        ),
        (
            "pascal status",
            serde_json::json!({"action": "create", "subject": "test", "status": "InProgress"}),
        ),
        (
            "kebab status",
            serde_json::json!({"action": "create", "subject": "test", "status": "in-progress"}),
        ),
    ];

    for (label, case) in serde_flexible_cases {
        assert!(
            serde_json::from_value::<TodoArgs>(case.clone()).is_ok(),
            "Serde flexible deserialization failed for '{label}': {case}"
        );
    }

    // Test executing tool directly with flexible args
    let tool = TodoTool::new(TodoStore::new());
    let res = tool
        .execute(
            serde_json::from_value(serde_json::json!({
                "action": "CREATE",
                "subject": "flexible task",
                "status": "InProgress",
                "active_form": "running tests"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(!res.is_error);
    assert!(res.content.contains("Created #1: flexible task (in_progress)"));

    let res2 = tool
        .execute(
            serde_json::from_value(serde_json::json!({
                "action": "update",
                "id": "1",
                "status": "completed"
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(!res2.is_error);
    assert!(res2.content.contains("Updated #1 (in_progress → completed)"));
}
