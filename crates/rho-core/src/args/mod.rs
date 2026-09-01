//! Host-side data shapes for built-in tool arguments. Tool implementations
//! own the execution; the floor classifies these shapes, and descriptors bind
//! schemas to them.

pub mod ask_user;
pub mod bash;
pub mod edit;
pub mod fetch;
pub mod read;
pub mod search;
pub mod write;

pub use ask_user::AskUserArgs;
pub use bash::BashArgs;
pub use edit::{EditArgs, EditReplacement};
pub use fetch::FetchArgs;
pub use read::ReadArgs;
pub use search::{SearchArgs, SearchRecency};
pub use write::WriteArgs;
