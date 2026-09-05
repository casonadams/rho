use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;

pub fn has_external_permission_plugin(plugins: &BTreeMap<String, PluginConfig>) -> bool {
    plugins.iter().any(|(name, cfg)| {
        if !cfg.enabled {
            return false;
        }
        if name == "permission" || name == "rho-plugin-permission" {
            return true;
        }
        if let Some(cmd) = &cfg.command
            && cmd.contains("rho-plugin-permission")
        {
            return true;
        }
        if cfg.path.to_string_lossy().contains("rho-plugin-permission") {
            return true;
        }
        if let Some(pkg) = &cfg.package
            && pkg.contains("rho-plugin-permission")
        {
            return true;
        }
        false
    })
}
