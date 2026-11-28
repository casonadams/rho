//! Spawning language server processes.

use super::config::ServerConfig;
use super::rpc::{BoxedReader, BoxedWriter};
use crate::locking::lock;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};

const STDERR_TAIL_BYTES: usize = 4096;

/// The last few KB a server wrote to stderr, to explain why it failed to start.
#[derive(Clone, Default)]
pub struct StderrTail(Arc<Mutex<String>>);

impl StderrTail {
    pub fn push_line(&self, line: &str) {
        let mut tail = lock(&self.0);
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
        let tail = lock(&self.0);
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

/// Whether an inherited variable looks like a credential a language server has no use for.
fn is_secret_var(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    const SUFFIXES: &[&str] = &[
        "_API_KEY",
        "_TOKEN",
        "_SECRET",
        "_PASSWORD",
        "_PASSPHRASE",
        "_CREDENTIALS",
    ];
    const EXACT: &[&str] = &["API_KEY", "SSH_AUTH_SOCK", "DATABASE_URL", "AWS_ACCESS_KEY_ID"];
    const PREFIXES: &[&str] = &["ANTHROPIC_", "OPENAI_", "GEMINI_", "GOOGLE_API", "AWS_SECRET", "AZURE_"];
    EXACT.contains(&name.as_str())
        || SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        || PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn scrub_secrets(cmd: &mut tokio::process::Command, inherited: impl IntoIterator<Item = std::ffi::OsString>) {
    for name in inherited {
        if name.to_str().is_some_and(is_secret_var) {
            cmd.env_remove(name);
        }
    }
}

/// The server's command: inherited credentials removed first, so `env` in the server's config can
/// still pass one on deliberately.
fn build_command(
    server: &ServerConfig,
    command: &Path,
    root: &Path,
    inherited: impl IntoIterator<Item = std::ffi::OsString>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(command);
    scrub_secrets(&mut cmd, inherited);
    cmd.args(&server.args)
        .current_dir(root)
        .envs(&server.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

impl Launcher for ProcessLauncher {
    fn launch(&self, server: &ServerConfig, command: &Path, root: &Path) -> Result<Launched, String> {
        let mut cmd = build_command(server, command, root, std::env::vars_os().map(|(name, _)| name));
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
            process: Some(Box::new(crate::process::ProcessTreeGuard::for_session(child))),
            stderr: tail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("ANTHROPIC_API_KEY"; "anthropic api key")]
    #[test_case("OPENAI_BASE_URL"; "openai base url")]
    #[test_case("GEMINI_API_KEY"; "gemini api key")]
    #[test_case("GOOGLE_API_KEY"; "google api key")]
    #[test_case("AWS_SECRET_ACCESS_KEY"; "aws secret access key")]
    #[test_case("AWS_SESSION_TOKEN"; "aws session token")]
    #[test_case("GITHUB_TOKEN"; "github token")]
    #[test_case("client_secret"; "lowercase client secret")]
    #[test_case("BRAVE_API_KEY"; "brave api key")]
    #[test_case("EXA_API_KEY"; "exa api key")]
    #[test_case("API_KEY"; "api key")]
    #[test_case("MYSQL_PASSWORD"; "mysql password")]
    #[test_case("GPG_PASSPHRASE"; "gpg passphrase")]
    #[test_case("SERVICE_CREDENTIALS"; "service credentials")]
    #[test_case("GOOGLE_APPLICATION_CREDENTIALS"; "google application credentials")]
    #[test_case("AWS_ACCESS_KEY_ID"; "aws access key id")]
    #[test_case("AZURE_CLIENT_ID"; "azure client id")]
    #[test_case("SSH_AUTH_SOCK"; "ssh auth sock")]
    #[test_case("DATABASE_URL"; "database url")]
    #[test_case("NPM_TOKEN"; "npm token")]
    fn credential_variables_are_recognized(secret: &str) {
        assert!(is_secret_var(secret));
    }

    #[test_case("PATH"; "path")]
    #[test_case("HOME"; "home")]
    #[test_case("RUSTUP_HOME"; "rustup home")]
    #[test_case("LANG"; "lang")]
    #[test_case("TERM"; "term")]
    #[test_case("GOPATH"; "gopath")]
    #[test_case("AWS_REGION"; "aws region")]
    #[test_case("KEYBOARD"; "keyboard")]
    fn ordinary_variables_are_kept(ordinary: &str) {
        assert!(!is_secret_var(ordinary));
    }

    #[cfg(unix)]
    #[test]
    fn the_spawned_command_loses_inherited_secrets_but_keeps_what_the_server_config_sets() {
        let mut server: ServerConfig = serde_json::from_value(serde_json::json!({ "command": "x" })).unwrap();
        server.env.insert("NPM_TOKEN".into(), "from-config".into());
        let inherited = ["HOME", "ANTHROPIC_API_KEY", "NPM_TOKEN", "SSH_AUTH_SOCK"].map(std::ffi::OsString::from);

        let cmd = build_command(&server, Path::new("/bin/x"), Path::new("/"), inherited);

        let envs: std::collections::HashMap<_, _> = cmd.as_std().get_envs().collect();
        assert_eq!(envs.get(std::ffi::OsStr::new("ANTHROPIC_API_KEY")), Some(&None));
        assert_eq!(envs.get(std::ffi::OsStr::new("SSH_AUTH_SOCK")), Some(&None));
        assert!(!envs.contains_key(std::ffi::OsStr::new("HOME")));
        assert_eq!(
            envs.get(std::ffi::OsStr::new("NPM_TOKEN")),
            Some(&Some(std::ffi::OsStr::new("from-config")))
        );
    }

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
