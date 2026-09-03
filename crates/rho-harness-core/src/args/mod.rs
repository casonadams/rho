//! Host-side data shapes for built-in tool arguments.

pub mod bash;
pub mod edit;
pub mod read;
pub mod web_fetch;
pub mod web_search;
pub mod write;

pub use bash::BashArgs;
pub use edit::{EditArgs, EditReplacement};
pub use read::ReadArgs;
pub use web_fetch::WebFetchArgs;
pub use web_search::{WebSearchArgs, WebSearchRecency};
pub use write::WriteArgs;
