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
    for key in cargo_build_vars() {
        cmd.env_remove(key);
    }
    cmd
}

/// The names of the inherited variables cargo set for the build of rho itself.
pub fn cargo_build_vars() -> impl Iterator<Item = OsString> {
    std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| is_cargo_build_var(key))
}

/// Variables cargo sets for the build of rho itself, which must not leak into commands the model runs.
fn is_cargo_build_var(key: &std::ffi::OsStr) -> bool {
    const PREFIXES: [&str; 4] = ["CARGO_PKG_", "CARGO_MANIFEST_", "CARGO_BIN_EXE_", "CARGO_CRATE_"];
    const NAMES: [&str; 3] = ["OUT_DIR", "CARGO_BIN_NAME", "CARGO_PRIMARY_PACKAGE"];
    let key = key.to_string_lossy();
    PREFIXES.iter().any(|prefix| key.starts_with(prefix)) || NAMES.contains(&key.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_build_variables_are_recognised() {
        for name in [
            "CARGO_PKG_NAME",
            "CARGO_MANIFEST_DIR",
            "OUT_DIR",
            "CARGO_CRATE_NAME",
            "CARGO_BIN_NAME",
            "CARGO_PRIMARY_PACKAGE",
            "CARGO_BIN_EXE_rho",
        ] {
            assert!(is_cargo_build_var(name.as_ref()), "{name}");
        }
        for name in ["PATH", "CARGO_HOME", "CARGO_TARGET_DIR", "RUSTUP_HOME"] {
            assert!(!is_cargo_build_var(name.as_ref()), "{name}");
        }
    }
}
