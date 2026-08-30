//! In-process evaluation harness for the agent runtime.
//!
//! The types are organised so that:
//!
//! - [`types`] holds the report/failure/normalized* data shapes
//!   that flow through `EvalHarness` and into reports.
//! - [`harness`] owns the orchestration: the `EvalHarness` entry
//!   point and the verify/normalize helpers.
//! - [`mock`] provides the `MockEngineConfig` / `mock_engine` helpers
//!   used to construct a real `AgentEngine` backed by a mocked LLM.
//! - [`context`] owns the bounded-history comparison machinery
//!   (separate concern from the rest of the harness).
//!
//! The `#[test]`/`#[tokio::test]` suite lives with the host workspace tests
//! (`rho/tests`) because it exercises real tools and the terminal renderer.

pub mod context;
pub mod presenter;

pub mod harness;

pub mod mock;

pub mod types;
