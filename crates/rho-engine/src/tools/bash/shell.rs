use std::path::Path;
use tokio::process::Command;

/// Resolves the shell executable and arguments for executing commands.
pub fn resolve_shell_command(command: &str) -> Command {
    #[cfg(unix)]
    {
        let shell = if Path::new("/bin/bash").exists() {
            "/bin/bash"
        } else if Path::new("/usr/bin/bash").exists() {
            "/usr/bin/bash"
        } else {
            "/bin/sh"
        };
        let mut cmd = Command::new(shell);
        cmd.arg("-c").arg(command);
        sanitize_cargo_env(&mut cmd);
        cmd
    }

    #[cfg(windows)]
    {
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

        let mut cmd = if let Some(bash) = git_bash {
            let mut c = Command::new(bash);
            c.arg("-c").arg(command);
            c
        } else {
            let mut c = Command::new("cmd.exe");
            c.arg("/C").arg(command);
            c
        };
        sanitize_cargo_env(&mut cmd);
        cmd
    }

    #[cfg(not(any(unix, windows)))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        sanitize_cargo_env(&mut cmd);
        cmd
    }
}

fn sanitize_cargo_env(cmd: &mut Command) {
    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_PKG_") || key.starts_with("CARGO_MANIFEST_") || key == "OUT_DIR" {
            cmd.env_remove(&key);
        }
    }
}
