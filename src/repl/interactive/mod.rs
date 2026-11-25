pub mod completion;
pub mod history;
pub mod models;
pub mod prediction;
pub mod sources;
pub mod words;

pub use completion::{CommandItem, Completion, CompletionSet, McpServerItem, McpServerList, ModelItem, SkillItem};
pub use history::InteractiveHistory;
pub use models::{ModelList, catalog_status, discover_models};
pub use prediction::predict_inline_completion;
pub use sources::CompletionSources;
pub use words::WordCompleter;

#[cfg(test)]
mod tests;
