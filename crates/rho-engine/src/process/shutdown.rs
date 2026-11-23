use super::kill_all_tracked_processes;

/// Stops every child rho started: language servers first (graceful), then all tracked process groups.
pub fn shutdown_children() {
    crate::lsp::shutdown_all_blocking();
    kill_all_tracked_processes();
}

/// Kills tracked process groups when any thread panics. Release builds abort on panic, so
/// neither `Drop` guards nor the normal exit path run afterwards.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        kill_all_tracked_processes();
        previous(info);
    }));
}
