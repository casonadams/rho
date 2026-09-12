//! rho agent runtime: turn orchestration, provider assembly, built-in tools,
//! permission gating, MCP integration, and lifecycle hooks.
//!
//! The central type is [`engine::AgentEngine`], which owns the agent loop
//! (prepare → provider stream → tool dispatch → completion), usage/quota
//! tracking, and auto-compaction. Everything here consumes the deterministic
//! domain types from `rho-harness-core`.
//!
//! Module map:
//! - `engine`: turn loop, runner sinks, compactor, context assembly, metrics.
//! - `provider`: per-provider model handle construction and model catalogs.
//! - `claude`, `antigravity`, `chatgpt`, `ollama`: provider wire protocols,
//!   SSE streaming, quota parsing, and OAuth flows (`auth`).
//! - `tools`: built-in tools (bash, read, edit, write, fd, rg, web) and
//!   shared plumbing (truncation, atomic writes, HTTP singletons).
//! - `permission`: bash analysis, policy evaluation, approval prompts.
//! - `mcp`: MCP client processes, transports, and tool gateway.
//! - `hook`: lifecycle hooks for turn and tool interception.
//! - `process`: process-group child management with cancellation safety.
//!
//! See `ARCHITECTURE.md` at the repository root for the execution loops.

pub mod antigravity;
pub mod auth;
pub mod chatgpt;
pub mod claude;
pub mod engine;
pub mod hook;
pub mod mcp;
pub mod ollama;
pub mod permission;
pub mod process;
pub mod provider;
pub mod repeat;
pub mod tools;

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
