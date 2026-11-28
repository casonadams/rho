use std::path::PathBuf;

fn rho_home_override() -> Option<PathBuf> {
    ["RHO_HOME", "RUST_AI_HOME"]
        .into_iter()
        .find_map(|var| std::env::var_os(var).map(PathBuf::from))
}

pub fn config_dir() -> Option<PathBuf> {
    rho_home_override().or_else(|| home_dir().map(|home| home.join(".config").join("rho")))
}

/// The directory standing in for `$HOME` when reading global MCP files: `RHO_HOME` when set, else the real home.
pub fn global_mcp_home() -> PathBuf {
    rho_home_override().unwrap_or_else(dirs_fallback)
}

pub fn default_config_dir() -> PathBuf {
    config_dir().unwrap_or_else(|| PathBuf::from(".").join(".config").join("rho"))
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn dirs_fallback() -> PathBuf {
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}
