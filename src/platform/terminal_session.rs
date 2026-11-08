//! Records which session an interactive terminal is running so tools such as mpx can resume
//! that exact session: `<config_dir>/terminal-sessions/<tty>` holds the session id.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

const DIR: &str = "terminal-sessions";

pub fn terminal_session_path(config_dir: &Path, tty: &str) -> Option<PathBuf> {
    let valid = !tty.is_empty() && !tty.contains('/') && tty != "." && tty != "..";
    valid.then(|| config_dir.join(DIR).join(tty))
}

pub fn write_terminal_session(config_dir: &Path, tty: &str, session_id: &str) -> io::Result<()> {
    let path = terminal_session_path(config_dir, tty)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid terminal name"))?;
    std::fs::create_dir_all(config_dir.join(DIR))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, format!("{session_id}\n"))?;
    std::fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Device name of the controlling terminal without `/dev/` (for example `ttys004`), or `None`
/// unless both stdin and stdout are terminals.
#[cfg(unix)]
pub fn interactive_tty_name() -> Option<String> {
    use std::os::fd::AsRawFd;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }
    let mut buf = [0 as libc::c_char; 128];
    // SAFETY: `buf` is valid for `buf.len()` bytes and ttyname_r NUL-terminates on success.
    let rc = unsafe { libc::ttyname_r(io::stdin().as_raw_fd(), buf.as_mut_ptr(), buf.len()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: a zero return means `buf` holds a NUL-terminated string.
    let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_str().ok()?;
    Some(name.strip_prefix("/dev/").unwrap_or(name).to_string())
}

#[cfg(not(unix))]
pub fn interactive_tty_name() -> Option<String> {
    None
}

/// Records `session_id` for the current terminal; does nothing when there is no interactive tty.
pub fn record_terminal_session(config_dir: &Path, session_id: &str) -> io::Result<()> {
    match interactive_tty_name() {
        Some(tty) => write_terminal_session(config_dir, &tty, session_id),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_session_id_and_replaces_it_on_a_later_switch() {
        let dir = tempfile::tempdir().unwrap();

        write_terminal_session(dir.path(), "ttys004", "20261108_192507_46b8ae52").unwrap();
        let path = terminal_session_path(dir.path(), "ttys004").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "20261108_192507_46b8ae52\n");

        write_terminal_session(dir.path(), "ttys004", "20261109_080000_deadbeef").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "20261109_080000_deadbeef\n");
        let leftovers = std::fs::read_dir(dir.path().join(DIR)).unwrap().count();
        assert_eq!(leftovers, 1, "temp files must not be left behind");
    }

    #[test]
    fn rejects_terminal_names_that_could_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();

        for tty in ["", ".", "..", "../ttys004", "pts/3"] {
            assert_eq!(terminal_session_path(dir.path(), tty), None, "{tty:?}");
            assert!(write_terminal_session(dir.path(), tty, "id").is_err(), "{tty:?}");
        }
        assert!(!dir.path().join(DIR).exists());
    }
}
