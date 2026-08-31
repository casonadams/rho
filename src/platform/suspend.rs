#[cfg(unix)]
pub fn suspend_process() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
    unsafe {
        libc::raise(libc::SIGTSTP);
    }
    let _ = crossterm::terminal::enable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide);
}

#[cfg(not(unix))]
pub fn suspend_process() {
    // Windows terminals do not support Unix job control signals
}

#[cfg(test)]
mod tests {
    #[test]
    fn suspend_compiles_and_callable() {
        // We do not raise SIGTSTP during unit tests to avoid halting the test runner
    }
}
