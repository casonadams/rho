//! Terminal and tmux titlebar publishing via OSC 0 (`\x1b]0;...\x07`).
//!
//! Pushes the host shell's title on startup (`\x1b[22;2t`) and pops it on clean
//! exit or abort (`\x1b[23;2t`), latching `disposed` beforehand so late async
//! updates never clobber the restored title.

use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::TerminalBackend;
use crate::ui::interactive::{Activity, InteractiveState};

pub(crate) const PUSH_TERMINAL_TITLE: &str = "\x1b[22;2t";
pub(crate) const POP_TERMINAL_TITLE: &str = "\x1b[23;2t";
const TITLE_BRAND: &str = "ρ";

static TERMINAL_TITLE_PUSHED: AtomicBool = AtomicBool::new(false);

/// Restores a pushed terminal title on panic or signal abort when the controller
/// cannot run its normal `Drop` path.
pub(crate) fn pop_title_on_abort(out: &mut impl Write) {
    if TERMINAL_TITLE_PUSHED.swap(false, Ordering::AcqRel) {
        let _ = out.write_all(POP_TERMINAL_TITLE.as_bytes());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleRunState {
    Idle,
    Working,
    Attention,
}

/// Strips C0/DEL/C1 control characters and collapses whitespace to prevent
/// escape injection in terminal title sequences.
pub fn sanitize_title_part(part: &str) -> String {
    let cleaned: String = part.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Picks the display label: explicit/auto session name when present, otherwise
/// the basename of the current working directory (omitting filesystem root).
pub fn label_from_footer(session_name: Option<&str>, cwd: Option<&str>) -> Option<String> {
    if let Some(name) = session_name {
        let sanitized = sanitize_title_part(name);
        if !sanitized.is_empty() {
            return Some(sanitized);
        }
    }
    let cwd = cwd?;
    let base = Path::new(cwd).file_name()?.to_str()?;
    let sanitized = sanitize_title_part(base);
    (!sanitized.is_empty()).then_some(sanitized)
}

pub fn format_terminal_title(label: Option<&str>, state: Option<TitleRunState>) -> String {
    match (state, label) {
        (None, Some(l)) => format!("{TITLE_BRAND}: {l}"),
        (None, None) => TITLE_BRAND.to_string(),
        (Some(TitleRunState::Idle), Some(l)) => format!("{TITLE_BRAND} · {l}"),
        (Some(TitleRunState::Idle), None) => format!("{TITLE_BRAND} ·"),
        (Some(TitleRunState::Working), Some(l)) => format!("{TITLE_BRAND} ⋯ {l}"),
        (Some(TitleRunState::Working), None) => format!("{TITLE_BRAND} ⋯"),
        (Some(TitleRunState::Attention), Some(l)) => format!("{TITLE_BRAND} ! {l}"),
        (Some(TitleRunState::Attention), None) => format!("{TITLE_BRAND} !"),
    }
}

pub fn format_osc0_title(title: &str) -> String {
    let clean = sanitize_title_part(title);
    format!("\x1b]0;{clean}\x07")
}

pub fn run_state_from_interactive(state: &InteractiveState, attention: bool) -> TitleRunState {
    if attention || state.footer().awaiting_approval.is_some() {
        TitleRunState::Attention
    } else if state.footer().activity != Activity::Idle {
        TitleRunState::Working
    } else {
        TitleRunState::Idle
    }
}

#[derive(Default)]
pub(super) struct TerminalTitleController {
    enabled: bool,
    show_state: bool,
    pushed: bool,
    disposed: bool,
    attention: bool,
    last_title: Option<String>,
}

impl TerminalTitleController {
    pub(super) fn configure<B: TerminalBackend>(
        &mut self,
        backend: &mut B,
        enabled: bool,
        show_state: bool,
        state: &InteractiveState,
    ) -> io::Result<()> {
        self.enabled = enabled;
        self.show_state = show_state;
        if !enabled || self.disposed {
            return Ok(());
        }
        if !self.pushed {
            backend.write_text(PUSH_TERMINAL_TITLE)?;
            self.pushed = true;
            TERMINAL_TITLE_PUSHED.store(true, Ordering::Release);
        }
        self.sync(backend, state)
    }

    pub(super) fn set_attention(&mut self, attention: bool) {
        self.attention = attention;
    }

    pub(super) fn sync<B: TerminalBackend>(&mut self, backend: &mut B, state: &InteractiveState) -> io::Result<()> {
        if !self.enabled || self.disposed {
            return Ok(());
        }
        let footer = state.footer();
        let label = label_from_footer(footer.session_name.as_deref(), footer.cwd.as_deref());
        let run_state = self
            .show_state
            .then(|| run_state_from_interactive(state, self.attention));
        let title = format_terminal_title(label.as_deref(), run_state);
        if self.last_title.as_deref() == Some(title.as_str()) {
            return Ok(());
        }
        backend.write_text(&format_osc0_title(&title))?;
        backend.flush()?;
        self.last_title = Some(title);
        Ok(())
    }

    pub(super) fn dispose<B: TerminalBackend>(&mut self, backend: &mut B) {
        self.disposed = true;
        self.last_title = None;
        if self.pushed {
            self.pushed = false;
            TERMINAL_TITLE_PUSHED.store(false, Ordering::Release);
            let _ = backend.write_text(POP_TERMINAL_TITLE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingBackend {
        written: String,
    }

    impl TerminalBackend for RecordingBackend {
        fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, text: &str) -> io::Result<()> {
            self.written.push_str(text);
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn sanitize_strips_control_chars_and_collapses_whitespace() {
        assert_eq!(
            sanitize_title_part("  hello\x1b]0;evil\x07\nworld\t "),
            "hello ]0;evil world"
        );
        assert_eq!(sanitize_title_part("\x00\x1f\x7f"), "");
    }

    #[test]
    fn label_prefers_session_name_then_cwd_basename() {
        assert_eq!(
            label_from_footer(Some("Fix Auth"), Some("/home/me/dev/rho")).as_deref(),
            Some("Fix Auth")
        );
        assert_eq!(
            label_from_footer(None, Some("/home/me/dev/rho")).as_deref(),
            Some("rho")
        );
        assert_eq!(label_from_footer(None, Some("/")), None);
        assert_eq!(label_from_footer(Some("   "), None), None);
    }

    #[test]
    fn format_terminal_title_handles_state_prefixes() {
        assert_eq!(format_terminal_title(Some("rho"), None), "ρ: rho");
        assert_eq!(format_terminal_title(None, None), "ρ");
        assert_eq!(format_terminal_title(Some("rho"), Some(TitleRunState::Idle)), "ρ · rho");
        assert_eq!(
            format_terminal_title(Some("rho"), Some(TitleRunState::Working)),
            "ρ ⋯ rho"
        );
        assert_eq!(
            format_terminal_title(Some("rho"), Some(TitleRunState::Attention)),
            "ρ ! rho"
        );
    }

    #[test]
    fn controller_pushes_dedups_updates_and_pops_on_dispose() {
        let mut backend = RecordingBackend::default();
        let mut state = InteractiveState::default();
        state.footer_mut().cwd = Some("/home/me/dev/rho".to_string());

        let mut title = TerminalTitleController::default();
        title.configure(&mut backend, true, true, &state).unwrap();
        assert_eq!(backend.written, format!("{PUSH_TERMINAL_TITLE}\x1b]0;ρ · rho\x07"));

        // Identical state is deduplicated.
        let before = backend.written.len();
        title.sync(&mut backend, &state).unwrap();
        assert_eq!(backend.written.len(), before);

        // Activity and session name changes emit updated OSC 0.
        state.footer_mut().activity = Activity::Working;
        state.footer_mut().session_name = Some("Auto title".to_string());
        title.sync(&mut backend, &state).unwrap();
        assert!(backend.written.ends_with("\x1b]0;ρ ⋯ Auto title\x07"));

        // Attention state takes precedence over working.
        title.set_attention(true);
        title.sync(&mut backend, &state).unwrap();
        assert!(backend.written.ends_with("\x1b]0;ρ ! Auto title\x07"));

        // Dispose pops saved title and blocks late writes.
        title.dispose(&mut backend);
        assert!(backend.written.ends_with(POP_TERMINAL_TITLE));
        let after_dispose = backend.written.len();
        state.footer_mut().session_name = Some("Late write".to_string());
        title.sync(&mut backend, &state).unwrap();
        assert_eq!(backend.written.len(), after_dispose);
    }

    #[test]
    fn disabled_controller_emits_nothing() {
        let mut backend = RecordingBackend::default();
        let state = InteractiveState::default();
        let mut title = TerminalTitleController::default();
        title.configure(&mut backend, false, true, &state).unwrap();
        title.sync(&mut backend, &state).unwrap();
        title.dispose(&mut backend);
        assert!(backend.written.is_empty());
    }
}
