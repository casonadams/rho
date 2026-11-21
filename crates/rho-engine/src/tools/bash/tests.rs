use super::*;
use crate::tools::truncate::DEFAULT_MAX_BYTES as MAX_BASH_BYTES;

fn bash_args(command: &str) -> BashArgs {
    BashArgs {
        command: command.to_string(),
        timeout: Some(10),
        background: None,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn the_model_gets_plain_text_while_the_colored_output_stays_for_the_display() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let streamed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let cases = [
        (
            "printf '\\033[31mred\\033[0m\\n'",
            false,
            "red",
            "\u{1b}[31mred\u{1b}[0m",
        ),
        (
            "printf '\\033[1;32mbad\\033[0m\\n'; exit 3",
            true,
            "bad\n\nCommand exited with code 3",
            "\u{1b}[1;32mbad\u{1b}[0m\n\nCommand exited with code 3",
        ),
    ];
    for (command, is_error, plain, colored) in cases {
        let sink = streamed.clone();
        let res = tool
            .execute_streaming(bash_args(command), move |chunk| sink.lock().unwrap().push_str(chunk))
            .await
            .unwrap();

        assert_eq!(res.is_error, is_error, "{command}");
        assert_eq!(res.content, plain, "{command}");
        assert_eq!(res.display.as_deref(), Some(colored), "{command}");
    }
    assert!(
        streamed.lock().unwrap().contains("\u{1b}[31mred"),
        "the live stream keeps its colors"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn output_without_colors_needs_no_separate_display_text() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool.execute(bash_args("echo plain")).await.unwrap();
    assert_eq!((res.content.as_str(), res.display), ("plain", None));
}

#[cfg(unix)]
#[tokio::test]
async fn a_bound_tool_owns_its_background_jobs_and_they_stop_when_the_session_is_abandoned() {
    let dir = tempfile::tempdir().unwrap();
    let owner = "bash-owner-7c2".to_string();
    let inbox = crate::mailbox::Mailboxes::default().open(&owner, None);
    let unbound = BashTool::new(dir.path());
    let tool = unbound
        .bind_turn(&TurnToolContext {
            inbox: Some(inbox.clone()),
            ..TurnToolContext::default()
        })
        .expect("a tool with a mailbox binds to it");
    assert!(unbound.bind_turn(&TurnToolContext::default()).is_none());

    let started = tool
        .execute(serde_json::json!({ "command": "sleep 30", "background": true }))
        .await
        .unwrap();
    let job_id = started
        .content
        .lines()
        .find_map(|line| line.strip_prefix("Job ID: "))
        .expect("the result names the job")
        .to_string();
    let jobs = crate::tools::jobs::JobManager::global();
    assert_eq!(jobs.get(&job_id).unwrap().owner, owner);
    assert!(jobs.has_running(&owner));

    crate::tools::jobs::abandon_background_work(&owner);

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while jobs.has_running(&owner) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the job stops promptly");
    assert!(matches!(
        jobs.get(&job_id).unwrap().status,
        crate::tools::jobs::JobStatus::Failed { .. }
    ));
    assert!(inbox.take().is_empty(), "a stopped job reports nothing");
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

#[test]
fn test_output_accumulator_creates_temp_file_when_lines_exceed_threshold() {
    let mut acc = OutputAccumulator::new();
    for i in 1..=20 {
        acc.append(format!("line {i}\n").as_bytes());
    }
    acc.finish();

    let snap = acc.snapshot();
    assert!(!snap.truncation.truncated);
    assert!(snap.full_output_path.is_some());
    let path = snap.full_output_path.unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("line 1\n"));
    assert!(content.contains("line 20\n"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_full_output_log_has_no_terminal_colors_even_when_escapes_span_chunks() {
    let mut acc = OutputAccumulator::new();
    acc.append(b"\x1b[31mline 1\x1b[0m\n");
    for i in 2..=20 {
        acc.append(format!("\x1b[1;3{}mline {i}\x1b[0m\n", i % 8).as_bytes());
    }
    acc.append(b"tail \x1b[3");
    acc.append(b"2mgreen\x1b[0m\n");
    acc.finish();

    let path = acc.snapshot().full_output_path.expect("over the line threshold");
    let log = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!log.contains('\u{1b}'), "{log:?}");
    assert!(log.starts_with("line 1\nline 2\n"), "{log:?}");
    assert!(log.ends_with("line 20\ntail green\n"), "{log:?}");
    assert!(
        acc.snapshot().formatted_text.contains("\x1b[32mgreen"),
        "the live text keeps its colors"
    );
}

#[test]
fn the_log_sweep_removes_only_old_rho_logs() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("rho-bash");
    std::fs::create_dir_all(&logs).unwrap();
    let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 60 * 60);
    let make = |path: std::path::PathBuf, old: bool| {
        let file = std::fs::File::create(&path).unwrap();
        if old {
            file.set_modified(day_ago).unwrap();
        }
        path
    };
    let old_log = make(logs.join("old.log"), true);
    let new_log = make(logs.join("new.log"), false);
    let old_loose = make(root.path().join("rho-bash-legacy.log"), true);
    let new_loose = make(root.path().join("rho-bash-current.log"), false);
    let foreign = make(root.path().join("someone-else.log"), true);

    assert_eq!(accumulator::sweep_logs(root.path()), 2);

    assert!(!old_log.exists() && !old_loose.exists());
    assert!(new_log.exists() && new_loose.exists() && foreign.exists());
}

#[test]
fn test_output_accumulator_preserves_ansi_escapes_split_across_chunks() {
    let mut acc = OutputAccumulator::new();
    acc.append(b"starting \x1b[4");
    acc.append(b"0mblack background\x1b[0m ending");
    acc.finish();

    let snap = acc.snapshot();
    assert_eq!(snap.formatted_text, "starting \x1b[40mblack background\x1b[0m ending");
}

#[tokio::test]
async fn test_bash_preserves_error_message_at_end_of_large_output() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "seq 1 5000; echo 'CRITICAL_FAILURE_AT_END'".to_string(),
            timeout: Some(10),
            background: None,
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
                background: None,
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
            background: None,
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
            background: None,
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
            background: None,
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
            background: Some(false),
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
        background: None,
    }));

    tokio::select! {
        _ = async {
            loop {
                if let Ok(c) = std::fs::read_to_string(&pid_file)
                    && !c.trim().is_empty()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        } => {}
        _ = &mut future => {}
    }

    drop(future);

    let mut content = String::new();
    for _ in 0..100 {
        if let Ok(c) = std::fs::read_to_string(&pid_file)
            && !c.trim().is_empty()
        {
            content = c;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
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
            background: None,
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
            background: None,
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
            command:
                "echo \"CI=$CI;GIT=$GIT_TERMINAL_PROMPT;PAGER=$PAGER;GIT_PAGER=$GIT_PAGER;CLICOLOR=$CLICOLOR;COLORTERM=$COLORTERM;TERM=$TERM\""
                    .to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(
        res.content
            .contains("CI=true;GIT=0;PAGER=cat;GIT_PAGER=cat;CLICOLOR=1;COLORTERM=truecolor;TERM=xterm-256color")
    );
}

#[tokio::test]
async fn test_bash_sanitizes_cargo_environment_variables() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "echo \"PKG=$CARGO_PKG_NAME;DIR=$CARGO_MANIFEST_DIR;OUT=$OUT_DIR\"".to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("PKG=;DIR=;OUT="));
}

#[tokio::test]
async fn test_bash_exit_zero_over_threshold_includes_metadata_footer() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "seq 1 25".to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    let footer = super::footer::BashFooter::parse(&res.content).expect("footer");
    assert_eq!(footer.lines, 25);
    let log = footer.log_path.expect("log path");
    assert!(log.contains("rho-bash"), "{log}");
    assert!(
        std::path::Path::new(&log)
            .file_stem()
            .is_some_and(|stem| stem.len() == 12),
        "{log}"
    );
}

#[tokio::test]
async fn test_bash_exit_zero_under_threshold_omits_metadata_footer() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "seq 1 5".to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(!res.content.contains("[exit 0;"));
}

#[tokio::test]
async fn test_bash_exit_zero_over_window_threshold_windows_output() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "seq 1 100".to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.starts_with("1\n2\n3"));
    assert!(res.content.contains("[... 75 lines omitted]"));
    assert!(res.content.contains("98\n99\n100"));
    assert_eq!(
        super::footer::BashFooter::parse(&res.content).expect("footer").lines,
        100
    );
}

#[tokio::test]
async fn test_bash_collapses_consecutive_identical_lines() {
    let tool = BashTool::new(std::env::current_dir().unwrap());
    let res = tool
        .execute(BashArgs {
            command: "printf 'repeat\\n%.0s' {1..10}".to_string(),
            timeout: Some(5),
            background: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("repeat\n[... 9 identical lines collapsed]"));
}

#[test]
fn test_output_accumulator_caps_full_log_size() {
    let mut acc = OutputAccumulator::new().with_log_limit(64);
    for _ in 0..20 {
        acc.append(b"0123456789abcdef\n");
    }
    acc.finish();

    let path = acc.snapshot().full_output_path.unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        content.len() <= 64 + "\n[log truncated at 64 bytes]\n".len(),
        "{}",
        content.len()
    );
    assert!(content.contains("[log truncated at 64 bytes]"));
}
