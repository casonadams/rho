use super::*;
use crate::tools::truncate::DEFAULT_MAX_BYTES as MAX_BASH_BYTES;

#[test]
fn test_is_read_only_command() {
    assert!(is_read_only_command("ls -la"));
    assert!(is_read_only_command("cat Cargo.toml && ls -la"));
    assert!(is_read_only_command("git status"));
    assert!(is_read_only_command("git diff"));
    assert!(is_read_only_command("cargo check"));
    assert!(is_read_only_command("cargo test"));
    assert!(is_read_only_command("rg 'fn main' src/"));

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
    assert!(is_read_only_command("git rev-parse HEAD"));
    assert!(is_read_only_command("git remote -v"));
    assert!(is_read_only_command("jq . package.json"));
    assert!(is_read_only_command("sort file.txt | uniq"));
    assert!(is_read_only_command("python3 --version"));
    assert!(is_read_only_command("node -v"));
    assert!(is_read_only_command("npm test"));
    assert!(is_read_only_command("go version"));
    assert!(!is_read_only_command("npm publish"));
    assert!(!is_read_only_command("python script.py"));
    assert!(!is_read_only_command("git remote add origin https://..."));
}

#[test]
fn test_output_accumulator_creates_temp_file_when_truncated() {
    let mut acc = OutputAccumulator::new();
    let chunk = vec![b'x'; MAX_BASH_BYTES + 500];
    acc.append(&chunk);
    acc.finish();

    let snap = acc.snapshot();
    assert!(snap.truncation.truncated);
    assert!(snap.full_output_path.is_some());
    let path = snap.full_output_path.unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn test_bash_preserves_error_message_at_end_of_large_output() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "seq 1 5000; echo 'CRITICAL_FAILURE_AT_END'".to_string(),
            timeout: Some(10),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("CRITICAL_FAILURE_AT_END"));
    assert!(res.content.contains("[Showing lines "));
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

#[cfg(unix)]
#[tokio::test]
async fn test_bash_timeout_kills_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid.txt");
    let tool = BashTool::new(std::env::current_dir().unwrap());

    let res = tool
        .execute(BashArgs {
            command: format!("echo $$ > '{}' && sleep 30 & wait", pid_file.display()),
            timeout: Some(1),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("timed out"));

    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("read pid file")
        .trim()
        .parse()
        .expect("parse pid");
    assert!(!crate::process::is_pid_tracked(pid));
    crate::process::wait_group_dead(pid).await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_bash_cancellation_kills_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid.txt");
    let tool = BashTool::new(std::env::current_dir().unwrap());

    let mut future = Box::pin(tool.execute(BashArgs {
        command: format!("echo $$ > '{}' && sleep 30 & wait", pid_file.display()),
        timeout: Some(30),
    }));

    tokio::select! {
        _ = async {
            while !pid_file.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        } => {}
        _ = &mut future => {}
    }

    drop(future);

    let content = std::fs::read_to_string(&pid_file).expect("read pid file");
    let pid: u32 = content.trim().parse().expect("parse pid");
    assert!(!crate::process::is_pid_tracked(pid));
    crate::process::wait_group_dead(pid).await;
}

#[tokio::test]
async fn test_bash_timeout_preserves_accumulated_output() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "echo 'early output'; sleep 3".to_string(),
            timeout: Some(1),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("early output"));
    assert!(res.content.contains("timed out after 1 seconds"));
}

#[tokio::test]
async fn test_bash_nonzero_exit_preserves_output_before_status() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "echo 'first output line'; exit 42".to_string(),
            timeout: Some(5),
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("first output line"));
    assert!(res.content.ends_with("Command exited with code 42"));
}

#[test]
fn test_accumulator_sanitizes_binary_output() {
    let mut acc = OutputAccumulator::new();
    acc.append(b"hello\x00\x07world\n");
    acc.finish();
    let snap = acc.snapshot();
    assert_eq!(snap.content, "helloworld\n");
}

#[tokio::test]
async fn test_bash_sets_noninteractive_env_safeguards() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "echo \"CI=$CI;GIT=$GIT_TERMINAL_PROMPT;PAGER=$PAGER\"".to_string(),
            timeout: Some(5),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("CI=true;GIT=0;PAGER=cat"));
}
