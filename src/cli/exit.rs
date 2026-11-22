//! Exit cleanup for SIGTERM, SIGHUP, and panics, which skip the normal return path and would
//! otherwise leave raw mode on and orphan servers running in their own sessions.

fn restore_terminal() {
    #[cfg(feature = "ui")]
    crate::ui::interactive::controller::backend::restore_terminal_after_abort();
}

/// Must run inside the tokio runtime. SIGINT is left alone: Esc cancels and Ctrl+C clears input.
pub fn install_exit_handlers() -> std::io::Result<()> {
    install_panic_hook();
    #[cfg(unix)]
    {
        let mut signals = unix::TerminationSignals::register()?;
        tokio::spawn(async move {
            let code = signals.recv().await;
            restore_terminal();
            rho_engine::process::shutdown_children();
            std::process::exit(code);
        });
    }
    Ok(())
}

fn install_panic_hook() {
    rho_engine::process::install_panic_hook();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

#[cfg(unix)]
mod unix {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    pub(super) struct TerminationSignals {
        term: Signal,
        hangup: Signal,
    }

    impl TerminationSignals {
        pub(super) fn register() -> std::io::Result<Self> {
            Ok(Self {
                term: signal(SignalKind::terminate())?,
                hangup: signal(SignalKind::hangup())?,
            })
        }

        /// Waits for SIGTERM or SIGHUP and returns the shell-convention exit code `128 + signal`.
        pub(super) async fn recv(&mut self) -> i32 {
            tokio::select! {
                _ = self.term.recv() => 128 + libc::SIGTERM,
                _ = self.hangup.recv() => 128 + libc::SIGHUP,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn termination_signals_map_to_shell_exit_codes() {
            let mut signals = TerminationSignals::register().expect("register handlers");
            for (sig, code) in [(libc::SIGHUP, 129), (libc::SIGTERM, 143)] {
                unsafe { libc::raise(sig) };
                assert_eq!(signals.recv().await, code);
            }
        }
    }
}
