#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Interface,
    Trait,
    Enum,
    Type,
    Impl,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Type => "type",
            Self::Impl => "impl",
        }
    }

    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.trim();
        if filter.is_empty() {
            return true;
        }
        self.as_str().eq_ignore_ascii_case(filter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub line: usize,
    pub depth: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum OutlineParseError {
    #[error("Tree-sitter parser error: {0}")]
    Parser(#[from] tree_sitter::LanguageError),
    #[error("Tree-sitter query error: {0}")]
    Query(#[from] tree_sitter::QueryError),
    #[error("Failed to parse source file")]
    FailedParse,
}
