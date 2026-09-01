//! Host-side data shapes for built-in tool arguments.

pub mod bash;
pub mod edit;
pub mod fetch;
pub mod read;
pub mod search;
pub mod write;

pub use bash::BashArgs;
pub use edit::{EditArgs, EditReplacement};
pub use fetch::FetchArgs;
pub use read::ReadArgs;
pub use search::{SearchArgs, SearchRecency};
pub use write::WriteArgs;
