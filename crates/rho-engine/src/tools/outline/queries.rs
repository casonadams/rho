use super::grammar::SupportedLanguage;
use tree_sitter::Query;

pub fn query_for_language(lang: SupportedLanguage) -> Result<Query, tree_sitter::QueryError> {
    let ts_lang = lang.tree_sitter_language();
    let query_str = query_string_for_language(lang);
    Query::new(&ts_lang, query_str)
}

pub fn query_string_for_language(lang: SupportedLanguage) -> &'static str {
    match lang {
        SupportedLanguage::Rust => RUST_QUERY,
        SupportedLanguage::TypeScript | SupportedLanguage::Tsx => TYPESCRIPT_QUERY,
        SupportedLanguage::JavaScript => JAVASCRIPT_QUERY,
        SupportedLanguage::Python => PYTHON_QUERY,
        SupportedLanguage::Go => GO_QUERY,
    }
}

pub(crate) static RUST_QUERY: &str = r#"
(function_item name: (identifier) @name) @function
(struct_item name: (type_identifier) @name) @struct
(enum_item name: (type_identifier) @name) @enum
(trait_item name: (type_identifier) @name) @trait
(type_item name: (type_identifier) @name) @type
(impl_item) @impl
"#;

pub(crate) static TYPESCRIPT_QUERY: &str = r#"
(function_declaration name: (_) @name) @function
(class_declaration name: (_) @name) @class
(interface_declaration name: (_) @name) @interface
(type_alias_declaration name: (_) @name) @type
(enum_declaration name: (_) @name) @enum
(method_definition name: (_) @name) @method
"#;

pub(crate) static JAVASCRIPT_QUERY: &str = r#"
(function_declaration name: (_) @name) @function
(class_declaration name: (_) @name) @class
(method_definition name: (_) @name) @method
"#;

pub(crate) static PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @name) @function
(class_definition name: (identifier) @name) @class
"#;

pub(crate) static GO_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @function
(method_declaration name: (field_identifier) @name) @method
(type_spec name: (type_identifier) @name) @type
(type_alias name: (type_identifier) @name) @type
"#;

#[cfg(test)]
#[path = "queries/tests.rs"]
mod tests;
