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

        if let Some(bash) = git_bash {
            let mut cmd = Command::new(bash);
            cmd.arg("-c").arg(command);
            cmd
        } else {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/C").arg(command);
            cmd
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}
