mod cli;

#[cfg(all(test, feature = "ui"))]
#[path = "tests/runner/mod.rs"]
mod runner_tests;

pub use rho_engine::{auth, engine, mcp, tools};
pub use rho_harness_core::{
    args, config, error, net, presentation, provider, queue, session, skills, tokens, workspace,
};
pub mod platform;

#[cfg(feature = "ui")]
pub mod repl;

#[cfg(feature = "ui")]
pub mod ui;

pub use cli::run_cli;
