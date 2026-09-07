//! Plugin management subsystem: spec parsing, install, update, remove, list.

pub mod archive;
pub mod install;
pub mod listing;
pub mod paths;
pub mod platform;
pub mod remove;
pub mod self_update;
pub mod spec;
pub mod update;

pub use archive::{ArchiveError, extract_binary, write_binary_atomically};
pub use install::{
    GitHubClient, GitHubError, InstallError, InstallPluginContext, InstallResult, Release, ReleaseAsset,
    handle_install, install_plugin,
};
pub use listing::{
    PluginListingItem, collect_plugin_listings, format_plugin_table, handle_inspect, handle_list, inspect_plugin,
};
pub use paths::{
    PluginEnvironment, expand_home, is_executable, is_in_cargo_bin, normalize_path, resolve_cargo_bin_dir,
    resolve_cargo_bin_dir_from, resolve_plugin_binary_path,
};
pub use platform::{Arch, Os, Platform, PlatformMatchError, match_platform_asset};
pub use remove::{
    PluginRemovalResult, RemovalArtifactStatus, RemovePluginContext, handle_remove, remove_plugin, resolve_plugin_key,
};
pub use self_update::{SelfUpdateContext, SelfUpdateStatus, handle_self_update, self_update};
pub use spec::{
    DEFAULT_GITHUB_ORG, DuplicatePluginError, PluginCandidate, PluginSpec, PluginSpecError, SimpleVersion,
    is_update_available, validate_no_duplicates,
};
pub use update::{PluginUpdateItem, PluginUpdateStatus, handle_update_all, handle_update_plugin, update_single_plugin};
