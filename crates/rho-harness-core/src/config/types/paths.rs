use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    for var in ["RHO_HOME", "RUST_AI_HOME"] {
        if let Ok(custom) = std::env::var(var) {
            return Some(PathBuf::from(custom));
        }
    }
    home_dir().map(|home| home.join(".config").join("rho"))
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
