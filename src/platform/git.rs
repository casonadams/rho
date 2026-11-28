//! Current git branch of a working directory, read from `.git` without spawning a process when possible.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const GIT_BRANCH_TIMEOUT: Duration = Duration::from_secs(3);

fn branch_from_head_file(head_file: &Path) -> Option<String> {
    let head_content = std::fs::read_to_string(head_file).ok()?;
    head_content.trim().strip_prefix("ref: refs/heads/").map(str::to_string)
}

fn branch_from_git_dir(dir: &Path, git_dir: &Path) -> Option<String> {
    let head_file = git_dir.join("HEAD");
    if git_dir.is_dir() {
        return branch_from_head_file(&head_file);
    }
    if git_dir.is_file() {
        let content = std::fs::read_to_string(git_dir).ok()?;
        let gitdir_path = content.trim().strip_prefix("gitdir:")?;
        let gitdir = PathBuf::from(gitdir_path.trim());
        let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
        return branch_from_head_file(&resolved.join("HEAD"));
    }
    None
}

fn branch_from_git_command(cmd: std::process::Command, timeout: Duration) -> Option<String> {
    let output = run_command_with_timeout(cmd, timeout)?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

fn branch_from_git_process(cwd: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["branch", "--show-current"]).current_dir(cwd);
    branch_from_git_command(cmd, GIT_BRANCH_TIMEOUT)
}

pub async fn get_git_branch_async(cwd: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || get_git_branch(&cwd))
        .await
        .ok()
        .flatten()
}

fn read_pipe(pipe: Option<impl std::io::Read>) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    if let Some(mut reader) = pipe {
        reader.read_to_end(&mut buf)?;
    }
    Ok(buf)
}

/// Waits up to `timeout` for `child` to exit, killing it when it does not; `None` when killed.
pub fn wait_for_child(child: &mut std::process::Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_and_reap(child);
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                kill_and_reap(child);
                return None;
            }
        }
    }
}

/// A failed kill or reap means the child already exited.
fn kill_and_reap(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub fn run_command_with_timeout(mut cmd: std::process::Command, timeout: Duration) -> Option<std::process::Output> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    std::thread::scope(|s| {
        let out_handle = s.spawn(|| read_pipe(stdout_pipe));
        let err_handle = s.spawn(|| read_pipe(stderr_pipe));
        let status = wait_for_child(&mut child, timeout)?;
        let stdout = out_handle.join().ok()?.ok()?;
        let stderr = err_handle.join().ok()?.ok()?;
        Some(std::process::Output { status, stdout, stderr })
    })
}

/// The branch named by the `HEAD` of the repository holding `cwd`, read from its files; `None` when
/// there is no `.git` above `cwd` or its `HEAD` names no branch, so only `git` itself can tell.
pub fn branch_from_files(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() || git_dir.is_file() {
            return branch_from_git_dir(dir, &git_dir);
        }
        curr = dir.parent();
    }
    None
}

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    branch_from_files(cwd).or_else(|| branch_from_git_process(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_from_git_command_gives_up_on_a_slow_git() {
        let mut slow = std::process::Command::new("sh");
        slow.args(["-c", "printf main; exec sleep 5"]);
        assert_eq!(branch_from_git_command(slow, Duration::from_millis(80)), None);
    }

    #[test]
    fn branch_from_git_command_reads_the_branch_name() {
        let mut fast = std::process::Command::new("sh");
        fast.args(["-c", "printf 'feat/x\\n'"]);
        assert_eq!(
            branch_from_git_command(fast, Duration::from_secs(2)),
            Some("feat/x".to_string())
        );
    }
}
