//! Antigravity (Google Cloud Code Assist) transport: endpoint/header helpers,
//! project/model discovery, the streaming rig client, and the Gemini-shaped
//! wire format used by the `v1internal` endpoints.

pub mod client;
pub mod quota;
pub mod request;
pub mod stream;

pub use client::{AntigravityClient, antigravity_headers, discover_models, into_handle, load_project_id};
pub use quota::fetch_quota;
pub use request::{Effort, RequestTarget, collapse_runtime_id, fallback_runtime_model, resolve_runtime_model};
