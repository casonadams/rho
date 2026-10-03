pub mod completion;
pub mod fuzzy;
pub mod history;
pub mod models;
pub mod prediction;
pub mod sources;

pub use completion::{CommandItem, Completion, CompletionSet, ModelItem, SkillItem};
pub use history::InteractiveHistory;
pub use models::{discover_models, spawn_background_model_refresh};
pub use prediction::predict_inline_completion;
pub use sources::CompletionSources;

#[cfg(test)]
mod tests;
