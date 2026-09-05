pub mod baseline;
pub mod bash;
pub mod eval;
pub mod external;
pub mod hook;
pub mod matcher;
pub mod path;
pub mod policy;
pub mod prompt;
pub mod suggest;
#[cfg(test)]
mod tests;
pub mod types;

pub use eval::{ask_drafts, decide_tool_call};
pub use external::has_external_permission_plugin;
pub use hook::PermissionHook;
pub use policy::{PermissionState, Policy, PolicyRule, load_policy, save_allow_rule};
pub use prompt::{build_permission_prompt, rewrite_tool_args};
pub use suggest::{canonical_tool, match_input, suggested_rule};
pub use types::{Decision, EvalRequest, RuleDraft};
