use super::*;

#[test]
fn test_is_read_only_command() {
    assert!(is_read_only_command("ls -la"));
    assert!(is_read_only_command("cat Cargo.toml && ls -la"));
    assert!(is_read_only_command("git status"));
    assert!(is_read_only_command("git diff"));
    assert!(is_read_only_command("cargo check"));
    assert!(is_read_only_command("cargo test"));
    assert!(is_read_only_command("rg 'fn main' src/"));

    // Dangerous / mutating commands must return false
    assert!(!is_read_only_command("rm -rf target"));
    assert!(!is_read_only_command("echo 'foo' > file.txt"));
    assert!(!is_read_only_command("git commit -m 'test'"));
    assert!(!is_read_only_command("npm install"));
    assert!(!is_read_only_command("cargo run"));
    assert!(!is_read_only_command("env sh -c 'touch marker'"));
    assert!(!is_read_only_command("git branch new-branch"));
    assert!(!is_read_only_command("git config user.name model"));
    assert!(is_read_only_command("git branch --show-current"));
    assert!(is_read_only_command("git config --get user.name"));
}

#[test]
fn test_bash_output_truncation_preserves_limits() {
    let output = format!("{}\n", "x".repeat(MAX_BASH_BYTES + 100));
    let truncated = truncate_bash_output(&output, output.len(), output.lines().count());
    assert!(truncated.contains("[Output truncated:"));
    assert!(truncated.len() <= MAX_BASH_BYTES + 100);
}

#[tokio::test]
async fn test_bash_retained_output_is_capped_while_streaming() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "yes | head -c 2000000".to_string(),
            timeout: Some(10),
        })
        .await
        .unwrap();
    assert!(!res.is_error);
    assert!(res.content.contains("[Output truncated: "));
    assert!(res.content.len() <= MAX_RETAINED_BASH_BYTES + 100);
}

#[tokio::test]
async fn test_bash_streaming_receives_chunks() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let r_clone = received.clone();

    let res = tool
        .execute_streaming(
            BashArgs {
                command: "echo 'first'; echo 'second'".to_string(),
                timeout: Some(5),
            },
            move |chunk| {
                r_clone.lock().unwrap().push(chunk.to_string());
            },
        )
        .await
        .unwrap();

    assert!(!res.is_error);
    let chunks = received.lock().unwrap().concat();
    assert!(chunks.contains("first"));
    assert!(chunks.contains("second"));
}

#[tokio::test]
async fn test_bash_echo() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "echo 'hello from bash'".to_string(),
            timeout: Some(5),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("hello from bash"));
}

#[tokio::test]
async fn test_bash_nonzero_exit() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "exit 42".to_string(),
            timeout: Some(5),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("exited with code 42"));
}

#[tokio::test]
async fn test_bash_timeout() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "sleep 3".to_string(),
            timeout: Some(1),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("timed out after 1 seconds"));
}
