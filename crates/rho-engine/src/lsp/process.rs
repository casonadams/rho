//! Spawning language server processes.

use super::config::ServerConfig;
use super::rpc::{BoxedReader, BoxedWriter};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncBufReadExt, BufReader};

const STDERR_TAIL_BYTES: usize = 4096;

/// The last few KB a server wrote to stderr, to explain why it failed to start.
#[derive(Clone, Default)]
pub struct StderrTail(Arc<Mutex<String>>);

impl StderrTail {
    pub fn push_line(&self, line: &str) {
        let mut tail = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        tail.push_str(line);
        tail.push('\n');
        if tail.len() > STDERR_TAIL_BYTES {
            let cut = tail.len() - STDERR_TAIL_BYTES;
            let cut = (cut..tail.len())
                .find(|&i| tail.is_char_boundary(i))
                .unwrap_or(tail.len());
            tail.drain(..cut);
        }
    }

    /// The last `lines` non-blank lines, joined with ` / `.
    pub fn last_lines(&self, lines: usize) -> String {
        let tail = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let kept: Vec<&str> = tail.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        kept[kept.len().saturating_sub(lines)..].join(" / ")
    }
}

pub struct Launched {
    pub reader: BoxedReader,
    pub writer: BoxedWriter,
    pub process: Option<Box<dyn Send + Sync>>,
    pub stderr: StderrTail,
}

/// Connects to a language server; tests substitute an in-process server.
pub trait Launcher: Send + Sync {
    fn launch(&self, server: &ServerConfig, command: &Path, root: &Path) -> Result<Launched, String>;
}

pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, server: &ServerConfig, command: &Path, root: &Path) -> Result<Launched, String> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&server.args)
            .current_dir(root)
            .envs(&server.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::process::isolate_group(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|error| format!("starting {}: {error}", server.name))?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(format!("{}: no stdio pipes", server.name));
        };
        let tail = StderrTail::default();
        let sink = tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                sink.push_line(&line);
            }
        });
        Ok(Launched {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            process: Some(Box::new(crate::process::ProcessTreeGuard::new(child))),
            stderr: tail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_tail_keeps_the_last_lines_within_its_budget() {
        let tail = StderrTail::default();
        for i in 0..2000 {
            tail.push_line(&format!("line {i}"));
        }
        tail.push_line("");
        assert_eq!(tail.last_lines(2), "line 1998 / line 1999");
        assert!(tail.0.lock().unwrap().len() <= STDERR_TAIL_BYTES);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_server_that_exits_at_once_leaves_its_stderr() {
        let mut server: ServerConfig = serde_json::from_value(
            serde_json::json!({ "command": "sh", "args": ["-c", "echo broken shim >&2; exit 3"] }),
        )
        .unwrap();
        server.name = "sh".into();
        let launched = ProcessLauncher
            .launch(&server, Path::new("/bin/sh"), Path::new("/"))
            .unwrap();
        let mut reader = launched.reader;
        let mut sink = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut sink)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while launched.stderr.last_lines(1).is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(launched.stderr.last_lines(1), "broken shim");
    }
}
