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

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
