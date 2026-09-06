use std::sync::LazyLock;
use tree_sitter::Query;

use super::grammar::SupportedLanguage;

fn compile(lang: SupportedLanguage) -> Result<Query, tree_sitter::QueryError> {
    let ts_lang = lang.tree_sitter_language();
    let query_str = query_string_for_language(lang);
    Query::new(&ts_lang, query_str)
}

static RUST_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Rust));
static TS_Q: LazyLock<Result<Query, tree_sitter::QueryError>> =
    LazyLock::new(|| compile(SupportedLanguage::TypeScript));
static TSX_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Tsx));
static JS_Q: LazyLock<Result<Query, tree_sitter::QueryError>> =
    LazyLock::new(|| compile(SupportedLanguage::JavaScript));
static PY_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Python));
static GO_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Go));
static JAVA_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Java));
static C_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::C));
static CPP_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Cpp));
static CSHARP_Q: LazyLock<Result<Query, tree_sitter::QueryError>> =
    LazyLock::new(|| compile(SupportedLanguage::CSharp));
static RUBY_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Ruby));
static PHP_Q: LazyLock<Result<Query, tree_sitter::QueryError>> = LazyLock::new(|| compile(SupportedLanguage::Php));

pub fn query_for_language(lang: SupportedLanguage) -> Result<&'static Query, &'static tree_sitter::QueryError> {
    let res = match lang {
        SupportedLanguage::Rust => &*RUST_Q,
        SupportedLanguage::TypeScript => &*TS_Q,
        SupportedLanguage::Tsx => &*TSX_Q,
        SupportedLanguage::JavaScript => &*JS_Q,
        SupportedLanguage::Python => &*PY_Q,
        SupportedLanguage::Go => &*GO_Q,
        SupportedLanguage::Java => &*JAVA_Q,
        SupportedLanguage::C => &*C_Q,
        SupportedLanguage::Cpp => &*CPP_Q,
        SupportedLanguage::CSharp => &*CSHARP_Q,
        SupportedLanguage::Ruby => &*RUBY_Q,
        SupportedLanguage::Php => &*PHP_Q,
    };
    res.as_ref()
}

pub fn query_string_for_language(lang: SupportedLanguage) -> &'static str {
    match lang {
        SupportedLanguage::Rust => RUST_QUERY,
        SupportedLanguage::TypeScript | SupportedLanguage::Tsx => TYPESCRIPT_QUERY,
        SupportedLanguage::JavaScript => JAVASCRIPT_QUERY,
        SupportedLanguage::Python => PYTHON_QUERY,
        SupportedLanguage::Go => GO_QUERY,
        SupportedLanguage::Java => JAVA_QUERY,
        SupportedLanguage::C => C_QUERY,
        SupportedLanguage::Cpp => CPP_QUERY,
        SupportedLanguage::CSharp => CSHARP_QUERY,
        SupportedLanguage::Ruby => RUBY_QUERY,
        SupportedLanguage::Php => PHP_QUERY,
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

pub(crate) static JAVA_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @class
(interface_declaration name: (identifier) @name) @interface
(record_declaration name: (identifier) @name) @class
(enum_declaration name: (identifier) @name) @enum
(method_declaration name: (identifier) @name) @method
(constructor_declaration name: (identifier) @name) @method
"#;

pub(crate) static C_QUERY: &str = r#"
(function_definition declarator: (_) @name) @function
(struct_specifier name: (type_identifier) @name) @struct
(union_specifier name: (type_identifier) @name) @struct
(enum_specifier name: (type_identifier) @name) @enum
(type_definition declarator: (type_identifier) @name) @type
"#;

pub(crate) static CPP_QUERY: &str = r#"
(class_specifier name: (type_identifier) @name) @class
(struct_specifier name: (type_identifier) @name) @struct
(enum_specifier name: (type_identifier) @name) @enum
(function_definition declarator: (_) @name) @function
(namespace_definition name: (namespace_identifier) @name) @type
"#;

pub(crate) static CSHARP_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @class
(interface_declaration name: (identifier) @name) @interface
(record_declaration name: (identifier) @name) @class
(struct_declaration name: (identifier) @name) @struct
(enum_declaration name: (identifier) @name) @enum
(method_declaration name: (identifier) @name) @method
(constructor_declaration name: (identifier) @name) @method
"#;

pub(crate) static RUBY_QUERY: &str = r#"
(class name: (constant) @name) @class
(module name: (constant) @name) @class
(method name: (identifier) @name) @method
(singleton_method name: (identifier) @name) @method
"#;

pub(crate) static PHP_QUERY: &str = r#"
(class_declaration name: (name) @name) @class
(interface_declaration name: (name) @name) @interface
(trait_declaration name: (name) @name) @trait
(enum_declaration name: (name) @name) @enum
(function_definition name: (name) @name) @function
(method_declaration name: (name) @name) @method
"#;

#[cfg(test)]
#[path = "queries/tests.rs"]
mod tests;
