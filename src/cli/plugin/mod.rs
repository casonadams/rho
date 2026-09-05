pub mod dedup;
pub mod listing;
pub mod paths;
pub mod platform;
pub mod remove;
pub mod spec;

pub use dedup::{DuplicatePluginError, PluginCandidate, validate_no_duplicates};
pub use listing::{
    PluginListingItem, collect_plugin_listings, format_plugin_table, handle_inspect, handle_list, inspect_plugin,
};
pub use paths::{
    expand_home, is_executable, is_in_cargo_bin, normalize_path, resolve_cargo_bin_dir, resolve_cargo_bin_dir_from,
    resolve_plugin_binary_path,
};
pub use platform::{Arch, Os, Platform, PlatformMatchError, match_platform_asset};
pub use remove::{
    PluginRemovalResult, RemovalArtifactStatus, RemovePluginContext, handle_remove, remove_plugin, resolve_plugin_key,
};
pub use spec::{DEFAULT_GITHUB_ORG, PluginSpec, PluginSpecError};
