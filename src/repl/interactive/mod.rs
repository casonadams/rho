pub mod completion;
pub mod fuzzy;
pub mod history;
pub mod models;
pub mod sources;

pub use completion::{Completion, CompletionSet, ModelItem, SkillItem};
pub use history::InteractiveHistory;
pub use models::{STANDARD_PRESETS, discover_models};
pub use sources::CompletionSources;

#[cfg(test)]
mod tests;
