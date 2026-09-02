use rho_harness_core::config::PluginConfig;
use std::path::{Path, PathBuf};

pub fn resolve_executable(plugin: &PluginConfig, working_dir: &Path) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(cmd) = &plugin.command {
        return Ok((PathBuf::from(cmd), plugin.args.clone()));
    }

    let path = if plugin.path.is_absolute() {
        plugin.path.clone()
    } else {
        working_dir.join(&plugin.path)
    };

    if path.is_file() {
        return Ok((path, plugin.args.clone()));
    }

    let release_bin = path
        .join("target/release")
        .join(plugin.path.file_name().unwrap_or_default());
    if release_bin.is_file() {
        return Ok((release_bin, plugin.args.clone()));
    }

    let debug_bin = path
        .join("target/debug")
        .join(plugin.path.file_name().unwrap_or_default());
    if debug_bin.is_file() {
        return Ok((debug_bin, plugin.args.clone()));
    }

    Ok((path, plugin.args.clone()))
}
