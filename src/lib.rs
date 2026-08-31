mod cli;

#[cfg(all(test, feature = "ui"))]
#[path = "tests/runner/mod.rs"]
mod runner_tests;

pub use rho_core::{
    approval, args, bash_ast, config, dispatch, error, net, policy, presentation, provider, queue, session, skills,
    tokens, workspace,
};
pub use rho_engine::{auth, engine};
pub mod platform;
pub mod plugin;

#[cfg(feature = "ui")]
pub mod repl;

pub mod tools;

#[cfg(feature = "ui")]
pub mod ui;

pub use cli::run_cli;
