use super::paths::{
    PluginEnvironment, is_executable, is_in_cargo_bin, resolve_cargo_bin_dir, resolve_plugin_binary_path,
};
use rho_harness_core::config::{Config, PluginConfig};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginListingItem {
    pub name: String,
    pub command_or_path: String,
    pub resolved_path: Option<PathBuf>,
    pub status: String,
    pub enabled: bool,
    pub managed: String,
}

fn resolve_plugin_status(path: Option<&PathBuf>) -> &'static str {
    match path {
        Some(p) if p.is_file() && is_executable(p) => "Installed (active)",
        Some(p) if p.is_file() => "Missing (not executable)",
        _ => "Missing",
    }
}

fn resolve_plugin_managed(path: Option<&PathBuf>, cargo_bin_dir: Option<&std::path::Path>) -> &'static str {
    match (path, cargo_bin_dir) {
        (Some(p), Some(bin)) if is_in_cargo_bin(p, bin) => "cargo-bin",
        _ => "system/local",
    }
}

pub fn inspect_plugin(name: &str, plugin: &PluginConfig, env: PluginEnvironment<'_>) -> PluginListingItem {
    let command_or_path = plugin
        .command
        .as_deref()
        .map(ToString::to_string)
        .unwrap_or_else(|| plugin.path.display().to_string());
    let resolved_path = resolve_plugin_binary_path(plugin, env.cargo_bin_dir, env.home_dir);
    let status = resolve_plugin_status(resolved_path.as_ref()).to_string();
    let managed = resolve_plugin_managed(resolved_path.as_ref(), env.cargo_bin_dir).to_string();

    PluginListingItem {
        name: name.to_string(),
        command_or_path,
        resolved_path,
        status,
        enabled: plugin.enabled,
        managed,
    }
}

pub fn collect_plugin_listings(
    plugins: &BTreeMap<String, PluginConfig>,
    env: PluginEnvironment<'_>,
) -> Vec<PluginListingItem> {
    plugins
        .iter()
        .map(|(name, cfg)| inspect_plugin(name, cfg, env))
        .collect()
}

struct TableWidths {
    w_name: usize,
    w_target: usize,
    w_status: usize,
    w_managed: usize,
}

fn compute_table_widths(items: &[PluginListingItem]) -> TableWidths {
    TableWidths {
        w_name: items.iter().map(|i| i.name.len()).max().unwrap_or(0).max("NAME".len()),
        w_target: items
            .iter()
            .map(|i| i.command_or_path.len())
            .max()
            .unwrap_or(0)
            .max("COMMAND / PATH".len()),
        w_status: items
            .iter()
            .map(|i| i.status.len())
            .max()
            .unwrap_or(0)
            .max("STATUS".len()),
        w_managed: items
            .iter()
            .map(|i| i.managed.len())
            .max()
            .unwrap_or(0)
            .max("MANAGED".len()),
    }
}

pub fn format_plugin_table(items: &[PluginListingItem]) -> String {
    if items.is_empty() {
        return "No plugins configured.\n".to_string();
    }

    let TableWidths {
        w_name,
        w_target,
        w_status,
        w_managed,
    } = compute_table_widths(items);
    let w_enabled = "ENABLED".len();

    let mut out = format!(
        "{:<w_name$}  {:<w_target$}  {:<w_status$}  {:<w_enabled$}  {:<w_managed$}\n",
        "NAME", "COMMAND / PATH", "STATUS", "ENABLED", "MANAGED"
    );

    for item in items {
        let enabled_str = if item.enabled { "yes" } else { "no" };
        out.push_str(&format!(
            "{:<w_name$}  {:<w_target$}  {:<w_status$}  {:<w_enabled$}  {:<w_managed$}\n",
            item.name, item.command_or_path, item.status, enabled_str, item.managed
        ));
    }

    out
}

pub fn handle_list(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let cargo_bin = resolve_cargo_bin_dir();
    let home = dirs::home_dir();
    let items = collect_plugin_listings(
        &config.plugins,
        PluginEnvironment {
            cargo_bin_dir: cargo_bin.as_deref(),
            home_dir: home.as_deref(),
        },
    );
    let table = format_plugin_table(&items);
    print!("{table}");
    Ok(())
}

pub fn handle_inspect(config: &Config, capability: Option<&str>) {
    println!("Configured MCP Servers & Plugins:");
    if config.mcp.servers.is_empty() && config.plugins.is_empty() {
        println!("  (none configured)");
    } else {
        for (name, server) in &config.mcp.servers {
            println!(
                "  - [mcp] {name}: command='{}' enabled={}",
                server.command, server.enabled
            );
        }
        for (name, plugin) in &config.plugins {
            let target = plugin
                .command
                .as_deref()
                .unwrap_or_else(|| plugin.path.to_str().unwrap_or(""));
            println!("  - [plugin] {name}: target='{target}' enabled={}", plugin.enabled);
        }
    }
    if let Some(cap) = capability {
        println!("\nInspecting capability '{cap}': (none matched)");
    }
}

#[cfg(test)]
#[path = "listing/tests.rs"]
mod tests;
