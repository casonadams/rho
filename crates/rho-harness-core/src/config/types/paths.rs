use std::path::PathBuf;

pub fn default_config_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RHO_HOME") {
        return PathBuf::from(custom);
    }
    if let Ok(custom) = std::env::var("RUST_AI_HOME") {
        return PathBuf::from(custom);
    }
    let home = dirs_fallback();
    home.join(".config").join("rho")
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn dirs_fallback() -> PathBuf {
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}
