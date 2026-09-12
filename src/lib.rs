//! rho CLI shell: subcommand dispatch, REPL frontends, and terminal UI.
//!
//! - `cli`: subcommand routing (`run`, `auth`, `mcp`, `update`, `rpc`), process
//!   cleanup guards, and session resume plumbing.
//! - `repl`: two frontends over one turn pipeline — `live` (raw-mode TUI with
//!   modals, streaming transcript, autocomplete) and `line_mode` (readline
//!   fallback) — coordinated through `coordinator`.
//! - `ui`: markdown rendering, the interactive controller (transcript cache,
//!   redraw batching, keymaps), and terminal paint primitives.
//! - `platform`: clipboard and terminal suspend helpers.
//!
//! The engine and domain live in `rho-engine` and `rho-harness-core`; this
//! crate is the I/O and interaction layer. See `ARCHITECTURE.md`.

pub mod cli;

#[cfg(all(test, feature = "ui"))]
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
