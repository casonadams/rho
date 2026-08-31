pub mod cargo;
pub mod manager;
#[cfg(test)]
mod tests;

pub use cargo::{CargoRunner, SystemCargo, default_cargo_bin};
pub use manager::{
    InstalledPlugin, PluginManager, PluginManagerPaths, PluginValidator, ProtocolPluginValidator, RemovedPlugin,
    validate_executable,
};
