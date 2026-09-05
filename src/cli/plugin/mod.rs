pub mod dedup;
pub mod platform;
pub mod spec;

pub use dedup::{DuplicatePluginError, PluginCandidate, validate_no_duplicates};
pub use platform::{Arch, Os, Platform, PlatformMatchError, match_platform_asset};
pub use spec::{DEFAULT_GITHUB_ORG, PluginSpec, PluginSpecError};
