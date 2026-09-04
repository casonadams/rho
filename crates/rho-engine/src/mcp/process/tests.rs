use super::*;

#[test]
fn test_resolve_env_with_indirection() {
    unsafe {
        std::env::set_var("TEST_MCP_SECRET_KEY", "secret_value_123");
    }
    let mut env = BTreeMap::new();
    env.insert("DIRECT".to_string(), "normal_val".to_string());
    env.insert("INDIRECT".to_string(), "env:TEST_MCP_SECRET_KEY".to_string());
    env.insert("MISSING".to_string(), "env:NONEXISTENT_VAR_XYZ".to_string());

    let resolved = resolve_env(&env);
    assert_eq!(resolved.get("DIRECT").map(|s| s.as_str()), Some("normal_val"));
    assert_eq!(resolved.get("INDIRECT").map(|s| s.as_str()), Some("secret_value_123"));
    assert!(!resolved.contains_key("MISSING"));
    unsafe {
        std::env::remove_var("TEST_MCP_SECRET_KEY");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_spawn_mcp_process() {
    let config = McpServerConfig {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), "sleep 30 & wait".to_string()],
        env: BTreeMap::new(),
        enabled: true,
    };
    let (_stdin, _stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
    let pid = handle.id().expect("handle has pid");
    assert!(pid > 1);
    drop(handle);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if unsafe { libc::kill(-(pid as libc::pid_t), 0) } == -1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("mcp process group {pid} still alive after handle drop");
}
