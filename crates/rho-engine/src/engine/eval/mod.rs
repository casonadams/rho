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
//!
//! The `#[test]`/`#[tokio::test]` suite lives with the host workspace tests
//! (`rho/tests`) because it exercises real tools and the terminal renderer.

pub mod presenter;

#[cfg(any(test, feature = "test-support"))]
pub mod harness;

#[cfg(any(test, feature = "test-support"))]
pub mod mock;

#[cfg(any(test, feature = "test-support"))]
pub mod types;
