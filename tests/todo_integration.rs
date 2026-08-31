#![cfg(unix)]

use rho::config::Config;
use rho::plugin::tool_dispatch::ActiveToolSet;
use rig::tool::ToolSet;
use std::path::PathBuf;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("todo_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_todo_tool_lifecycle_via_active_tool_set() {
    let workspace = temp_workspace();
    let config = Config::default();
    let active = ActiveToolSet::load(&config, &workspace).await.unwrap();

    let names: Vec<String> = active.definitions().iter().map(|d| d.id.name().to_string()).collect();
    assert!(names.contains(&"todo".to_string()));

    let tools = ToolSet::from_dynamic_tools(active.into_rig_tools());
    let mut context = rig::tool::ToolContext::new();

    // 1. Create task 1
    let res = tools
        .execute(
            "todo",
            r#"{"action":"create","subject":"Plan architecture","description":"Break down design"}"#,
            &mut context,
        )
        .await;
    assert!(res.is_success());
    assert!(
        res.output()
            .as_text()
            .unwrap()
            .contains("Created #1: Plan architecture (pending)")
    );

    // 2. Create task 2 with dependency
    let res = tools
        .execute(
            "todo",
            r#"{"action":"create","subject":"Implement core logic","blockedBy":[1]}"#,
            &mut context,
        )
        .await;
    assert!(res.is_success());
    assert!(
        res.output()
            .as_text()
            .unwrap()
            .contains("Created #2: Implement core logic (pending)")
    );

    // 3. List tasks
    let res = tools.execute("todo", r#"{"action":"list"}"#, &mut context).await;
    assert!(res.is_success());
    let list_text = res.output().as_text().unwrap();
    assert!(list_text.contains("#1: [pending] Plan architecture"));
    assert!(list_text.contains("#2: [pending] Implement core logic (blocked by #1)"));

    // 4. Update task 1 to in_progress with activeForm
    let res = tools
        .execute(
            "todo",
            r#"{"action":"update","id":1,"status":"in_progress","activeForm":"drafting spec"}"#,
            &mut context,
        )
        .await;
    assert!(res.is_success());
    assert!(
        res.output()
            .as_text()
            .unwrap()
            .contains("Updated #1 (pending → in_progress)")
    );

    // 5. Get task 1 details
    let res = tools.execute("todo", r#"{"action":"get","id":1}"#, &mut context).await;
    assert!(res.is_success());
    let get_text = res.output().as_text().unwrap();
    assert!(get_text.contains("Subject: Plan architecture"));
    assert!(get_text.contains("Status: in_progress"));
    assert!(get_text.contains("Active form: drafting spec"));

    // 6. Complete task 1
    let res = tools
        .execute(
            "todo",
            r#"{"action":"update","id":1,"status":"completed"}"#,
            &mut context,
        )
        .await;
    assert!(res.is_success());
    assert!(
        res.output()
            .as_text()
            .unwrap()
            .contains("Updated #1 (in_progress → completed)")
    );

    // 7. Delete task 2
    let res = tools
        .execute("todo", r#"{"action":"delete","id":2}"#, &mut context)
        .await;
    assert!(res.is_success());
    assert!(
        res.output()
            .as_text()
            .unwrap()
            .contains("Deleted #2: Implement core logic")
    );

    // 8. List non-deleted tasks
    let res = tools.execute("todo", r#"{"action":"list"}"#, &mut context).await;
    assert!(res.is_success());
    let list_text = res.output().as_text().unwrap();
    assert!(list_text.contains("#1: [completed] Plan architecture"));
    assert!(!list_text.contains("#2:"));

    // 9. Clear all tasks
    let res = tools.execute("todo", r#"{"action":"clear"}"#, &mut context).await;
    assert!(res.is_success());
    assert!(res.output().as_text().unwrap().contains("Cleared all tasks."));

    let _ = std::fs::remove_dir_all(workspace);
}
