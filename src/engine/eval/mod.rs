//! In-process evaluation harness for the agent runtime.
//!
//! All types and helpers here are test-only; the module is gated
//! `#[cfg(test)]` in `engine/mod.rs`. The types are organised so that:
//!
//! - [`types`] holds the report/failure/normalized* data shapes
//!   that flow through `EvalHarness` and into reports.
//! - [`harness`] owns the orchestration: the `EvalHarness` entry
//!   point and the verify/normalize helpers.
//! - [`mock`] provides the `MockEngineConfig` / `mock_engine` helpers
//!   used to construct a real `AgentEngine` backed by a mocked LLM.
//! - [`context`] owns the bounded-history comparison machinery
//!   (separate concern from the rest of the harness).
//! - [`tests`] is the actual `#[test]` / `#[tokio::test]` suite.

#[cfg(test)]
mod types;

#[cfg(test)]
mod harness;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod context;

#[cfg(test)]
mod tests;
