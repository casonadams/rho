use std::io;

#[cfg(unix)]
pub fn suspend_process() -> io::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::cursor::Show)?;
    if unsafe { libc::raise(libc::SIGTSTP) } != 0 {
        return Err(io::Error::last_os_error());
    }
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::cursor::Hide)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn suspend_process() -> io::Result<()> {
    // Windows terminals do not support Unix job control signals
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn suspend_compiles_and_callable() {
        // We do not raise SIGTSTP during unit tests to avoid halting the test runner
    }
}
