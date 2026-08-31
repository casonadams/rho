use rho_core::error::{AppError, Result};
use std::path::PathBuf;

pub trait CargoRunner: Send + Sync {
    fn install(&self, package: &str) -> Result<()>;
    fn uninstall(&self, package: &str) -> Result<()>;
}

pub struct SystemCargo;

pub fn default_cargo_bin() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(PathBuf::from(root).join("bin"));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".cargo").join("bin"))
        .ok_or_else(|| AppError::Plugin("Cannot determine Cargo installation directory".to_string()))
}

impl CargoRunner for SystemCargo {
    fn install(&self, package: &str) -> Result<()> {
        run_cargo(["install", package])
    }

    fn uninstall(&self, package: &str) -> Result<()> {
        run_cargo(["uninstall", package])
    }
}

fn run_cargo<const N: usize>(arguments: [&str; N]) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(arguments)
        .status()
        .map_err(|error| AppError::Plugin(format!("Failed to run Cargo: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Plugin(format!("Cargo exited with status {status}")))
    }
}
