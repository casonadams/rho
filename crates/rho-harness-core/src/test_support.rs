use std::path::PathBuf;

pub(crate) fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rho_test_{}", uuid::Uuid::new_v4()))
}

pub(crate) fn created_temp_dir() -> PathBuf {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
