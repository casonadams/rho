pub mod eval;
pub mod io;
pub mod model;
pub mod parse;

pub use eval::{build_policy, decide_surface};
pub use io::{
    config_dir, config_is_healthy, config_path, load_policy, project_config_path, read_scope_file, save_allow_rule,
    target_config_path,
};
pub use model::{PermissionState, Policy, PolicyRule, ScopeRules, SurfaceDecision, SurfaceKind};
pub use parse::parse_scope_from_str;
