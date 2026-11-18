use crate::engine::runner::turn::types::CancellationSignal;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;

const MAX_OUTPUT_CHARS: usize = 20_000;
pub(crate) const CANCELLED: &str = "cancelled by operator";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutcome {
    pub(crate) fn ok(&self) -> bool {
        self.exit == Some(0)
    }

    pub(crate) fn combined(&self) -> String {
        let text = match (self.stdout.trim_end(), self.stderr.trim_end()) {
            (out, "") => out.to_string(),
            ("", err) => err.to_string(),
            (out, err) => format!("{out}\n{err}"),
        };
        tail(&text, MAX_OUTPUT_CHARS)
    }

    pub(crate) fn to_value(&self, json: Option<Value>) -> Value {
        let mut value = json!({ "ok": self.ok(), "exit": self.exit, "output": self.combined() });
        if let (Some(json), Some(map)) = (json, value.as_object_mut()) {
            map.insert("json".to_string(), json);
        }
        value
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().skip(total - max_chars).collect();
    format!("[... {} characters omitted]\n{kept}", total - max_chars)
}

pub(crate) async fn run_command(
    args: &[String],
    cwd: &Path,
    cancel: Option<&CancellationSignal>,
) -> Result<CommandOutcome, String> {
    let (program, rest) = args.split_first().ok_or("cannot run an empty command")?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(rest)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|e| format!("failed to start `{program}`: {e}"))?;
    let output = child.wait_with_output();
    let output = match cancel {
        Some(cancel) => tokio::select! {
            out = output => out,
            () = cancel.wait_cancelled() => return Err(CANCELLED.to_string()),
        },
        None => output.await,
    }
    .map_err(|e| format!("`{program}` failed: {e}"))?;
    Ok(CommandOutcome {
        exit: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

async fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let argv: Vec<String> = std::iter::once("git")
        .chain(args.iter().copied())
        .map(str::to_string)
        .collect();
    let outcome = run_command(&argv, cwd, None).await?;
    if outcome.ok() {
        Ok(outcome.stdout.trim().to_string())
    } else {
        Err(format!("`git {}` failed: {}", args.join(" "), outcome.combined()))
    }
}

pub(crate) async fn start_branch(cwd: &Path, workflow: &str) -> Result<String, String> {
    let status = git(cwd, &["status", "--porcelain"]).await?;
    if !status.is_empty() {
        return Err(format!(
            "workflow '{workflow}' runs on its own branch and needs a clean working tree; commit or stash these changes first:\n{status}"
        ));
    }
    let branch = format!("rho/{workflow}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    git(cwd, &["switch", "--quiet", "-c", &branch]).await?;
    Ok(branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| (*p).to_string()).collect()
    }

    #[tokio::test]
    async fn runs_without_a_shell_and_reports_exit_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let ok = run_command(&argv(&["echo", "a; rm -rf /", "$HOME"]), dir.path(), None)
            .await
            .unwrap();
        assert!(ok.ok());
        assert_eq!(
            ok.stdout, "a; rm -rf / $HOME\n",
            "arguments reach the program literally"
        );

        let failed = run_command(&argv(&["sh", "-c", "echo out; echo err >&2; exit 3"]), dir.path(), None)
            .await
            .unwrap();
        assert_eq!(
            failed.to_value(None),
            json!({ "ok": false, "exit": 3, "output": "out\nerr" })
        );
        let missing = run_command(&argv(&["definitely-not-a-program-rho"]), dir.path(), None).await;
        assert!(missing.unwrap_err().contains("failed to start"));
    }

    #[tokio::test]
    async fn cancellation_stops_a_running_command() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationSignal::default();
        cancel.cancel();
        let err = run_command(&argv(&["sleep", "5"]), dir.path(), Some(&cancel))
            .await
            .unwrap_err();
        assert_eq!(err, CANCELLED);
    }

    #[test]
    fn tail_keeps_the_end_of_long_output() {
        let text = format!("{}END", "a".repeat(MAX_OUTPUT_CHARS));
        let cut = tail(&text, MAX_OUTPUT_CHARS);
        assert!(cut.starts_with("[... 3 characters omitted]"));
        assert!(cut.ends_with("END"));
    }

    #[tokio::test]
    async fn branch_isolation_needs_a_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        let git_cmd = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
        };
        git_cmd(&["init", "--quiet", "-b", "main"]);
        git_cmd(&[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "init",
        ]);
        std::fs::write(dir.path().join("dirty.rs"), "x").unwrap();
        let err = start_branch(dir.path(), "refine").await.unwrap_err();
        assert!(
            err.contains("needs a clean working tree") && err.contains("dirty.rs"),
            "{err}"
        );
        std::fs::remove_file(dir.path().join("dirty.rs")).unwrap();
        assert!(
            start_branch(dir.path(), "refine")
                .await
                .unwrap()
                .starts_with("rho/refine-")
        );
    }
}
