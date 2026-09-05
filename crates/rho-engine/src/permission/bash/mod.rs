pub mod analysis;
pub(crate) mod filter;
pub mod format;
pub mod lexer;
pub(crate) mod operator;
pub mod token;
pub(crate) mod word;

pub use analysis::{BashAnalysis, analyze_bash_command};
pub use filter::has_file_redirection;
pub use format::format_command_lines;
