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
            let code = unix::exit_on_signal(
                &mut signals,
                rho_engine::process::shutdown_children,
                rho_engine::process::kill_all_tracked_processes,
            )
            .await;
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
        hangup: Option<Signal>,
    }

    impl TerminationSignals {
        /// SIGHUP stays ignored when the process was started that way (`nohup`, a detached rpc server).
        pub(super) fn register() -> std::io::Result<Self> {
            Ok(Self {
                term: signal(SignalKind::terminate())?,
                hangup: register_unless_ignored(SignalKind::hangup())?,
            })
        }

        /// Waits for SIGTERM or SIGHUP and returns the shell-convention exit code `128 + signal`.
        pub(super) async fn recv(&mut self) -> i32 {
            tokio::select! {
                _ = self.term.recv() => 128 + libc::SIGTERM,
                () = recv_or_pend(self.hangup.as_mut()) => 128 + libc::SIGHUP,
            }
        }
    }

    async fn recv_or_pend(signal: Option<&mut Signal>) {
        match signal {
            Some(signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    }

    fn register_unless_ignored(kind: SignalKind) -> std::io::Result<Option<Signal>> {
        if is_ignored(kind.as_raw_value()) {
            return Ok(None);
        }
        signal(kind).map(Some)
    }

    fn is_ignored(signal: libc::c_int) -> bool {
        // SAFETY: a null new action only queries the current disposition into the zeroed struct.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            libc::sigaction(signal, std::ptr::null(), &mut action) == 0 && action.sa_sigaction == libc::SIG_IGN
        }
    }

    /// Runs `shutdown` after the first signal and returns the exit code to use. `shutdown` can take
    /// seconds, so a second signal meanwhile runs `force` and exits at once with that signal's code.
    pub(super) async fn exit_on_signal(
        signals: &mut TerminationSignals,
        shutdown: impl FnOnce() + Send + 'static,
        force: impl FnOnce(),
    ) -> i32 {
        let first = signals.recv().await;
        super::restore_terminal();
        let shutdown = tokio::task::spawn_blocking(shutdown);
        tokio::select! {
            _ = shutdown => first,
            second = signals.recv() => {
                force();
                second
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;

        #[test]
        fn a_signal_set_to_ignore_is_not_registered() {
            let kind = SignalKind::user_defined2();
            let raw = kind.as_raw_value();
            let mut observed = Vec::new();
            for handler in [libc::SIG_IGN, libc::SIG_DFL] {
                unsafe { libc::signal(raw, handler) };
                observed.push((is_ignored(raw), tokio_registers(kind)));
            }
            assert_eq!(observed, [(true, false), (false, true)]);
        }

        fn tokio_registers(kind: SignalKind) -> bool {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _enter = runtime.enter();
            register_unless_ignored(kind).unwrap().is_some()
        }

        #[tokio::test]
        async fn termination_signals_map_to_shell_exit_codes_and_a_second_one_forces_exit() {
            let mut signals = TerminationSignals::register().expect("register handlers");
            let hangup_ignored = signals.hangup.is_none();
            for (sig, code) in [(libc::SIGHUP, 129), (libc::SIGTERM, 143)] {
                if sig == libc::SIGHUP && hangup_ignored {
                    continue;
                }
                unsafe { libc::raise(sig) };
                assert_eq!(signals.recv().await, code);
            }

            let (started_tx, started_rx) = mpsc::channel::<()>();
            let (release_tx, release_rx) = mpsc::channel::<()>();
            let forced = std::sync::Arc::new(AtomicBool::new(false));
            let force_flag = forced.clone();
            unsafe { libc::raise(libc::SIGTERM) };
            let exit = tokio::spawn(async move {
                let code = exit_on_signal(
                    &mut signals,
                    move || {
                        started_tx.send(()).unwrap();
                        let _ = release_rx.recv();
                    },
                    move || force_flag.store(true, Ordering::SeqCst),
                )
                .await;
                (code, signals)
            });
            tokio::task::spawn_blocking(move || started_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(!forced.load(Ordering::SeqCst));

            unsafe { libc::raise(libc::SIGTERM) };
            let (code, _signals) = exit.await.unwrap();
            release_tx.send(()).unwrap();

            assert_eq!(code, 143);
            assert!(forced.load(Ordering::SeqCst));
        }
    }
}
