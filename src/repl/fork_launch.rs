use std::path::Path;

use crate::repl::commands::types::ForkTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkLaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub label: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MuxEnv {
    pub tmux: Option<String>,
    pub zellij: Option<String>,
    pub term_program: Option<String>,
    pub kitty_window_id: Option<String>,
    pub wezterm_pane: Option<String>,
    pub ghostty_resources_dir: Option<String>,
    pub is_macos: bool,
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl MuxEnv {
    pub fn from_process() -> Self {
        Self {
            tmux: non_empty_env("TMUX"),
            zellij: non_empty_env("ZELLIJ"),
            term_program: non_empty_env("TERM_PROGRAM"),
            kitty_window_id: non_empty_env("KITTY_WINDOW_ID"),
            wezterm_pane: non_empty_env("WEZTERM_PANE"),
            ghostty_resources_dir: non_empty_env("GHOSTTY_RESOURCES_DIR"),
            is_macos: cfg!(target_os = "macos"),
        }
    }
}

fn unsupported_fork_message(session_id: &str) -> String {
    format!(
        "Forked session {session_id}, but no supported multiplexer or terminal was detected. \
         `/fork <target>` requires tmux, Zellij, WezTerm, Kitty, or macOS Terminal/iTerm2. \
         Resume manually with: rho --resume {session_id}"
    )
}

fn push_cwd_and_resume(
    args: &mut Vec<String>,
    cwd_flag: &str,
    cwd: &Path,
    exe: &Path,
    session_id: &str,
    separator: bool,
) {
    args.push(cwd_flag.to_string());
    args.push(cwd.to_string_lossy().into_owned());
    if separator {
        args.push("--".to_string());
    }
    args.push(exe.to_string_lossy().into_owned());
    args.push("--resume".to_string());
    args.push(session_id.to_string());
}

fn tmux_plan(target: ForkTarget, exe: &Path, session_id: &str, cwd: &Path) -> ForkLaunchPlan {
    let (subcmd, split_flag, label) = match target {
        ForkTarget::Pane => ("split-window", Some("-h"), "tmux pane"),
        ForkTarget::HSplit => ("split-window", Some("-h"), "tmux hsplit"),
        ForkTarget::VSplit => ("split-window", Some("-v"), "tmux vsplit"),
        ForkTarget::Window => ("new-window", None, "tmux window"),
        ForkTarget::Tab => ("new-window", None, "tmux tab"),
    };
    let mut args = vec![subcmd.to_string()];
    args.extend(split_flag.map(str::to_string));
    push_cwd_and_resume(&mut args, "-c", cwd, exe, session_id, true);
    ForkLaunchPlan {
        program: "tmux".to_string(),
        args,
        label,
    }
}

fn zellij_plan(target: ForkTarget, exe: &Path, session_id: &str, cwd: &Path) -> ForkLaunchPlan {
    let (action, direction, label) = match target {
        ForkTarget::Pane => ("new-pane", None, "Zellij pane"),
        ForkTarget::HSplit => ("new-pane", Some("right"), "Zellij hsplit"),
        ForkTarget::VSplit => ("new-pane", Some("down"), "Zellij vsplit"),
        ForkTarget::Window => ("new-tab", None, "Zellij window"),
        ForkTarget::Tab => ("new-tab", None, "Zellij tab"),
    };
    let mut args = vec!["action".to_string(), action.to_string()];
    if let Some(dir) = direction {
        args.push("--direction".to_string());
        args.push(dir.to_string());
    }
    push_cwd_and_resume(&mut args, "--cwd", cwd, exe, session_id, true);
    ForkLaunchPlan {
        program: "zellij".to_string(),
        args,
        label,
    }
}

fn wezterm_plan(target: ForkTarget, exe: &Path, session_id: &str, cwd: &Path) -> ForkLaunchPlan {
    let (subcmd, dir_flag, label) = match target {
        ForkTarget::Pane => ("split-pane", None, "WezTerm pane"),
        ForkTarget::HSplit => ("split-pane", Some("--right"), "WezTerm hsplit"),
        ForkTarget::VSplit => ("split-pane", Some("--bottom"), "WezTerm vsplit"),
        ForkTarget::Window => ("spawn", None, "WezTerm window"),
        ForkTarget::Tab => ("spawn", None, "WezTerm tab"),
    };
    let mut args = vec!["cli".to_string(), subcmd.to_string()];
    args.extend(dir_flag.map(str::to_string));
    push_cwd_and_resume(&mut args, "--cwd", cwd, exe, session_id, true);
    ForkLaunchPlan {
        program: "wezterm".to_string(),
        args,
        label,
    }
}

fn kitty_plan(target: ForkTarget, exe: &Path, session_id: &str, cwd: &Path) -> ForkLaunchPlan {
    let (launch_type, label) = match target {
        ForkTarget::Window | ForkTarget::Tab => ("--type=tab", "Kitty tab"),
        ForkTarget::Pane | ForkTarget::HSplit | ForkTarget::VSplit => ("--type=window", "Kitty pane"),
    };
    let mut args = vec!["@".to_string(), "launch".to_string(), launch_type.to_string()];
    push_cwd_and_resume(&mut args, "--cwd", cwd, exe, session_id, false);
    ForkLaunchPlan {
        program: "kitten".to_string(),
        args,
        label,
    }
}

fn shell_quote(raw: &str) -> String {
    let escaped = raw.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn applescript_escape(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn iterm_plan(target: ForkTarget, shell_cmd: &str) -> ForkLaunchPlan {
    let escaped = applescript_escape(shell_cmd);
    let (action, label) = match target {
        ForkTarget::Window => ("create window", "iTerm2 window"),
        ForkTarget::Tab => ("tell current window to create tab", "iTerm2 tab"),
        ForkTarget::Pane | ForkTarget::HSplit => (
            "tell current session of current window to split vertically",
            "iTerm2 split",
        ),
        ForkTarget::VSplit => (
            "tell current session of current window to split horizontally",
            "iTerm2 vsplit",
        ),
    };
    let script = format!("tell application \"iTerm\" to {action} with default profile command \"{escaped}\"");
    ForkLaunchPlan {
        program: "osascript".to_string(),
        args: vec!["-e".to_string(), script],
        label,
    }
}

fn apple_terminal_plan(shell_cmd: &str) -> ForkLaunchPlan {
    let escaped = applescript_escape(shell_cmd);
    let script = format!("tell application \"Terminal\" to do script \"{escaped}\"");
    ForkLaunchPlan {
        program: "osascript".to_string(),
        args: vec!["-e".to_string(), script],
        label: "Terminal.app",
    }
}

fn macos_plan(
    target: ForkTarget,
    exe: &Path,
    session_id: &str,
    cwd: &Path,
    env: &MuxEnv,
) -> Result<ForkLaunchPlan, String> {
    let shell_cmd = format!(
        "cd {} && {} --resume {}",
        shell_quote(&cwd.to_string_lossy()),
        shell_quote(&exe.to_string_lossy()),
        shell_quote(session_id)
    );
    match env.term_program.as_deref() {
        Some("iTerm.app") => Ok(iterm_plan(target, &shell_cmd)),
        Some("Apple_Terminal") => Ok(apple_terminal_plan(&shell_cmd)),
        _ if env.ghostty_resources_dir.is_some() || env.term_program.as_deref() == Some("ghostty") => Err(format!(
            "Forked session {session_id}, but Ghostty does not support remote pane/tab spawning. \
             Resume manually with: rho --resume {session_id}"
        )),
        _ => Err(unsupported_fork_message(session_id)),
    }
}

pub fn build_fork_launch_plan(
    target: ForkTarget,
    exe: &Path,
    session_id: &str,
    cwd: &Path,
    env: &MuxEnv,
) -> Result<ForkLaunchPlan, String> {
    if env.tmux.is_some() {
        return Ok(tmux_plan(target, exe, session_id, cwd));
    }
    if env.zellij.is_some() {
        return Ok(zellij_plan(target, exe, session_id, cwd));
    }
    if env.wezterm_pane.is_some() || env.term_program.as_deref() == Some("WezTerm") {
        return Ok(wezterm_plan(target, exe, session_id, cwd));
    }
    if env.kitty_window_id.is_some() || env.term_program.as_deref() == Some("kitty") {
        return Ok(kitty_plan(target, exe, session_id, cwd));
    }
    if env.is_macos {
        return macos_plan(target, exe, session_id, cwd, env);
    }
    Err(unsupported_fork_message(session_id))
}

pub fn spawn_forked_session(target: ForkTarget, session_id: &str, cwd: &Path) -> Result<&'static str, String> {
    let exe = std::env::current_exe().map_err(|err| {
        format!(
            "Forked session {session_id}, but failed to resolve current executable: {err}. \
             Resume manually with: rho --resume {session_id}"
        )
    })?;
    let env = MuxEnv::from_process();
    let plan = build_fork_launch_plan(target, &exe, session_id, cwd, &env)?;
    let status = std::process::Command::new(&plan.program)
        .args(&plan.args)
        .status()
        .map_err(|err| {
            format!(
                "Forked session {session_id}, but failed to launch {} ({}): {err}. \
                 Resume manually with: rho --resume {session_id}",
                plan.label, plan.program
            )
        })?;
    if !status.success() {
        return Err(format!(
            "Forked session {session_id}, but {} exited with {status}. \
             Resume manually with: rho --resume {session_id}",
            plan.label
        ));
    }
    Ok(plan.label)
}

pub fn fork_status_message(session_id: &str, target: Option<ForkTarget>, fallback_cwd: &Path) -> String {
    let Some(target) = target else {
        return format!("Forked session: {session_id}");
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| fallback_cwd.to_path_buf());
    match spawn_forked_session(target, session_id, &cwd) {
        Ok(label) => format!("Forked session {session_id} in {label}"),
        Err(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> (&'static Path, &'static Path) {
        (Path::new("/usr/local/bin/rho"), Path::new("/tmp/my workspace"))
    }

    #[test]
    fn tmux_launch_plans_cover_all_targets() {
        let (exe, cwd) = test_paths();
        let env = MuxEnv {
            tmux: Some("/tmp/tmux-1000/default,123,0".into()),
            wezterm_pane: Some("1".into()),
            ..MuxEnv::default()
        };

        let pane = build_fork_launch_plan(ForkTarget::Pane, exe, "s1", cwd, &env).unwrap();
        assert_eq!(pane.program, "tmux");
        assert_eq!(
            pane.args,
            [
                "split-window",
                "-h",
                "-c",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s1"
            ]
        );

        let hsplit = build_fork_launch_plan(ForkTarget::HSplit, exe, "s1", cwd, &env).unwrap();
        assert_eq!(hsplit.args[0..2], ["split-window", "-h"]);

        let vsplit = build_fork_launch_plan(ForkTarget::VSplit, exe, "s1", cwd, &env).unwrap();
        assert_eq!(vsplit.args[0..2], ["split-window", "-v"]);

        let window = build_fork_launch_plan(ForkTarget::Window, exe, "s1", cwd, &env).unwrap();
        assert_eq!(
            window.args,
            [
                "new-window",
                "-c",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s1"
            ]
        );

        let tab = build_fork_launch_plan(ForkTarget::Tab, exe, "s1", cwd, &env).unwrap();
        assert_eq!(tab.args[0], "new-window");
    }

    #[test]
    fn zellij_launch_plans_cover_all_targets() {
        let (exe, cwd) = test_paths();
        let env = MuxEnv {
            zellij: Some("0".into()),
            ..MuxEnv::default()
        };

        let pane = build_fork_launch_plan(ForkTarget::Pane, exe, "s2", cwd, &env).unwrap();
        assert_eq!(pane.program, "zellij");
        assert_eq!(
            pane.args,
            [
                "action",
                "new-pane",
                "--cwd",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s2"
            ]
        );

        let hsplit = build_fork_launch_plan(ForkTarget::HSplit, exe, "s2", cwd, &env).unwrap();
        assert_eq!(
            hsplit.args,
            [
                "action",
                "new-pane",
                "--direction",
                "right",
                "--cwd",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s2"
            ]
        );

        let vsplit = build_fork_launch_plan(ForkTarget::VSplit, exe, "s2", cwd, &env).unwrap();
        assert_eq!(vsplit.args[0..4], ["action", "new-pane", "--direction", "down"]);

        let tab = build_fork_launch_plan(ForkTarget::Tab, exe, "s2", cwd, &env).unwrap();
        assert_eq!(
            tab.args,
            [
                "action",
                "new-tab",
                "--cwd",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s2"
            ]
        );
    }

    #[test]
    fn wezterm_launch_plans_work_via_pane_or_term_program() {
        let (exe, cwd) = test_paths();
        let env = MuxEnv {
            wezterm_pane: Some("42".into()),
            ..MuxEnv::default()
        };
        let pane = build_fork_launch_plan(ForkTarget::Pane, exe, "s3", cwd, &env).unwrap();
        assert_eq!(pane.program, "wezterm");
        assert_eq!(
            pane.args,
            [
                "cli",
                "split-pane",
                "--cwd",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s3"
            ]
        );

        let hsplit = build_fork_launch_plan(ForkTarget::HSplit, exe, "s3", cwd, &env).unwrap();
        assert_eq!(hsplit.args[0..3], ["cli", "split-pane", "--right"]);

        let vsplit = build_fork_launch_plan(ForkTarget::VSplit, exe, "s3", cwd, &env).unwrap();
        assert_eq!(vsplit.args[0..3], ["cli", "split-pane", "--bottom"]);

        let env_term = MuxEnv {
            term_program: Some("WezTerm".into()),
            ..MuxEnv::default()
        };
        let tab = build_fork_launch_plan(ForkTarget::Tab, exe, "s3", cwd, &env_term).unwrap();
        assert_eq!(
            tab.args,
            [
                "cli",
                "spawn",
                "--cwd",
                "/tmp/my workspace",
                "--",
                "/usr/local/bin/rho",
                "--resume",
                "s3"
            ]
        );
    }

    #[test]
    fn kitty_launch_plans_work_via_window_id_or_term_program() {
        let (exe, cwd) = test_paths();
        let env = MuxEnv {
            kitty_window_id: Some("7".into()),
            ..MuxEnv::default()
        };
        let tab = build_fork_launch_plan(ForkTarget::Tab, exe, "s4", cwd, &env).unwrap();
        assert_eq!(tab.program, "kitten");
        assert_eq!(
            tab.args,
            [
                "@",
                "launch",
                "--type=tab",
                "--cwd",
                "/tmp/my workspace",
                "/usr/local/bin/rho",
                "--resume",
                "s4"
            ]
        );

        let env_term = MuxEnv {
            term_program: Some("kitty".into()),
            ..MuxEnv::default()
        };
        let pane = build_fork_launch_plan(ForkTarget::Pane, exe, "s4", cwd, &env_term).unwrap();
        assert_eq!(pane.args[0..3], ["@", "launch", "--type=window"]);
    }

    #[test]
    fn macos_iterm_and_terminal_launch_plans_use_osascript() {
        let (exe, cwd) = test_paths();
        let iterm = MuxEnv {
            is_macos: true,
            term_program: Some("iTerm.app".into()),
            ..MuxEnv::default()
        };
        let plan = build_fork_launch_plan(ForkTarget::Tab, exe, "s5", cwd, &iterm).unwrap();
        assert_eq!(plan.program, "osascript");
        assert_eq!(plan.args[0], "-e");
        assert!(plan.args[1].contains("create tab with default profile command"));
        assert!(plan.args[1].contains("'/usr/local/bin/rho' --resume 's5'"));

        let term = MuxEnv {
            is_macos: true,
            term_program: Some("Apple_Terminal".into()),
            ..MuxEnv::default()
        };
        let term_plan = build_fork_launch_plan(ForkTarget::Window, exe, "s5", cwd, &term).unwrap();
        assert_eq!(term_plan.program, "osascript");
        assert!(term_plan.args[1].contains("tell application \"Terminal\" to do script"));
    }

    #[test]
    fn unsupported_and_ghostty_envs_return_actionable_errors_with_resume_hint() {
        let (exe, cwd) = test_paths();
        let plain = MuxEnv::default();
        let err = build_fork_launch_plan(ForkTarget::Pane, exe, "sess-99", cwd, &plain).unwrap_err();
        assert!(err.contains("tmux"));
        assert!(err.contains("Zellij"));
        assert!(err.contains("rho --resume sess-99"));

        let ghostty = MuxEnv {
            is_macos: true,
            ghostty_resources_dir: Some("/Applications/Ghostty.app/Contents/Resources".into()),
            ..MuxEnv::default()
        };
        let ghostty_err = build_fork_launch_plan(ForkTarget::Tab, exe, "sess-100", cwd, &ghostty).unwrap_err();
        assert!(ghostty_err.contains("Ghostty"));
        assert!(ghostty_err.contains("rho --resume sess-100"));
    }
}
