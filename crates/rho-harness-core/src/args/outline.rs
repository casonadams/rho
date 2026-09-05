use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct OutlineArgs {
    /// Relative or absolute path to a file or directory to outline
    pub path: String,
    /// Optional symbol name query (case-insensitive substring match)
    pub query: Option<String>,
    /// Optional filter by symbol kind ('function', 'method', 'struct', 'class', 'interface', 'trait', 'enum', 'type')
    pub kind: Option<String>,
    /// Optional maximum symbol nesting depth (default 2, max 5)
    pub depth: Option<usize>,
}
