//! Declarative agent workflows: TOML step lists that the engine executes deterministically.

pub mod bundle;
pub mod parse;
pub mod scopes;
pub mod template;
pub mod types;

pub use bundle::{WorkflowBundle, WorkflowCatalog, WorkflowOrigin};
pub use parse::parse_workflow;
pub use scopes::{Scope, default_max_scopes, plan_scopes, reviewable_dir_counts};
pub use types::{
    AgentStep, Builtin, Check, InputSpec, Isolation, OnFail, OutputFormat, Repeat, Step, StepKind, Workflow,
    WorkflowCall,
};
