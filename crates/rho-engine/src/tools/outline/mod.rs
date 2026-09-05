pub mod classify;
pub mod format;
pub mod grammar;
pub mod parser;
pub mod queries;
pub mod search;
pub mod signature;
pub mod types;

pub use format::{FileOutline, format_outlines};
pub use grammar::{SupportedLanguage, create_parser, detect_language};
pub use parser::parse_symbols;
pub use search::{OutlineSearchOptions, search_outline};
pub use types::{OutlineParseError, SymbolEntry, SymbolKind};
