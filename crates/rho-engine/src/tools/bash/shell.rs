use std::ffi::OsString;
use std::path::Path;
use tokio::process::Command;

#[cfg(unix)]
fn default_unix_shell() -> &'static str {
    if Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else if Path::new("/usr/bin/bash").exists() {
        "/usr/bin/bash"
    } else {
        "/bin/sh"
    }
}

#[cfg(windows)]
fn windows_shell_argv(command: &str) -> Vec<OsString> {
    let git_bash = std::env::var("ProgramFiles")
        .map(|p| format!(r"{p}\Git\bin\bash.exe"))
        .ok()
        .filter(|p| Path::new(p).exists())
        .or_else(|| {
            std::env::var("ProgramFiles(x86)")
                .map(|p| format!(r"{p}\Git\bin\bash.exe"))
                .ok()
                .filter(|p| Path::new(p).exists())
        });
    match git_bash {
        Some(bash) => vec![bash.into(), "-c".into(), command.into()],
        None => vec!["cmd.exe".into(), "/C".into(), command.into()],
    }
}

/// The shell program and arguments that run `command`, starting with the program.
pub fn shell_argv(command: &str) -> Vec<OsString> {
    #[cfg(unix)]
    let argv = vec![default_unix_shell().into(), "-c".into(), command.into()];

    #[cfg(windows)]
    let argv = windows_shell_argv(command);

    #[cfg(not(any(unix, windows)))]
    let argv = vec!["sh".into(), "-c".into(), command.into()];

    argv
}

/// Resolves the shell executable and arguments for executing commands.
pub fn resolve_shell_command(command: &str) -> Command {
    let argv = shell_argv(command);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    for key in std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| is_cargo_build_var(key))
    {
        cmd.env_remove(key);
    }
    cmd
}

/// Variables cargo sets for the build of rho itself, which must not leak into commands the model runs.
pub fn is_cargo_build_var(key: &std::ffi::OsStr) -> bool {
    let key = key.to_string_lossy();
    key.starts_with("CARGO_PKG_") || key.starts_with("CARGO_MANIFEST_") || key == "OUT_DIR"
}
