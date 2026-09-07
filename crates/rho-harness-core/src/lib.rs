//! rho host domain: deterministic workspace, session, config, and provider
//! identity primitives shared by every crate.
//!
//! This crate is the foundation of the dependency graph and must stay free of
//! UI framework types, HTTP wiring, and model-runtime details. It owns:
//! - `config`: layered configuration (defaults → file → env → CLI).
//! - `session`: durable JSONL session storage with history tree, branching,
//!   checkpointing, compaction bookkeeping, and secret redaction.
//! - `workspace`: path containment and mutation guards.
//! - `tokens`: token estimation and context cut-point selection.
//! - `presentation`: UI-agnostic display contracts (`Presenter`, tool lines,
//!   structured NDJSON output).
//! - `args`, `auth`, `error`, `net`, `prompts`, `provider`, `queue`, `rpc`,
//!   `skills`: built-in tool argument schemas, credential storage, error
//!   types, URL safety, prompt templates, provider identity, message
//!   queueing, RPC transport, and skill discovery.
//!
//! See `ARCHITECTURE.md` at the repository root for the full map.

pub mod args;
pub mod auth;
pub mod config;
pub mod error;
pub mod net;
pub mod presentation;
pub mod prompts;
pub mod provider;
pub mod queue;
pub mod rpc;
pub mod session;
pub mod skills;
pub mod tokens;
pub mod workspace;
